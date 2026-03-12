use libafl::corpus::{Corpus, InMemoryCorpus, OnDiskCorpus};
use libafl::events::SimpleEventManager;
use libafl::executors::ForkserverExecutor;
use libafl::feedbacks::{MaxMapFeedback, TimeFeedback, TimeoutFeedback};
use libafl::inputs::BytesInput;
use libafl::monitors::SimpleMonitor;
use libafl::mutators::{ScheduledMutator, havoc_mutations};
use libafl::observers::{CanTrack, HitcountsMapObserver, StdMapObserver, TimeObserver};
use libafl::schedulers::{IndexesLenTimeMinimizerScheduler, QueueScheduler};
use libafl::stages::StdMutationalStage;
use libafl::state::{HasCorpus, StdState};
use libafl::{Error, Fuzzer, StdFuzzer, feedback_and_fast, feedback_or};
use libafl_bolts::rands::StdRand;
use libafl_bolts::shmem::{ShMem, ShMemProvider, StdShMemProvider};
use libafl_bolts::tuples::tuple_list;
use libafl_bolts::{AsSliceMut, current_nanos};
use std::path::PathBuf;
use std::time::Duration;

/// size of the shared mapping used as the coverage map
const MAP_SIZE: usize = 65536;

fn main() {
    //
    // Component: Corpus
    //
    // path to input corpus
    let corpus_dir = vec![PathBuf::from("./corpus")];

    // Corpus that will be evolved, we keep it in memory for performance
    let input_corpus = InMemoryCorpus::<BytesInput>::new();

    // Corpus in which we store solutions (timeouts/hangs in this example).
    // on disk so the user can get them after stopping the fuzzer
    let timeouts_corpus =
        OnDiskCorpus::new(PathBuf::from("./timeouts")).expect("Could not create timeout corpus");

    //
    // Component: Observer
    //
    // Creates an observer channel to keep track of the current testcase's execution time.
    let time_observer = TimeObserver::new("time");

    // Create an observation channel using the coverage map.
    //
    // The ForkserverExecutor gets a pointer to shared memory from the _AFL_SHM_ID environment
    // variable.
    //
    // further explanation from toka: the edges map pointed by _AFL_SHM_ID is inserted by
    // afl-clang-fast, if you use afl-clang-fast, you can use _AFL_SHM_ID to get the ptr to the
    // map
    // The shmem provider supported by AFL++ for shared memory
    let mut shmem_provider = StdShMemProvider::new().unwrap();

    // The coverage map shared between observer and executor
    let mut shmem = shmem_provider.new_shmem(MAP_SIZE).unwrap();

    // let the forkserver know the shmid
    unsafe {
        shmem
            .write_to_env("__AFL_SHM_ID")
            .expect("Couldn't write shared memory ID");
    }

    let shmem_map = shmem.as_slice_mut();

    // Create an observation channel using signals map
    let edges_observer = unsafe {
        HitcountsMapObserver::new(StdMapObserver::new("shared_mem", shmem_map)).track_indices()
    };

    //
    // Component: Feedback
    //

    // A Feedback, in most cases, processes the information reported by one or more observer to
    // decide if the execution is interesting. This one is composed of two Feedbacks using a logical
    // OR.
    //
    // Due to the fact that TimeFeedback can never classify a testcase as interesting on its own,
    // we need to use it alongside some other Feedback that has the ability to perform said
    // classification. These two feedbacks are combined to create a boolean formula, i.e. if the input
    // triggered a new code path OR, false.
    let mut feedback = feedback_or!(
        // New maximization map feedback (attempts to maximize the map contents) linked to the
        // edges observer
        MaxMapFeedback::new(&edges_observer),
        // Time feedback, this one never returns true for is_interesting. However, it does keep
        // track of testcase execution time by way of its TimeObserver
        TimeFeedback::new(&time_observer)
    );

    // A feedback is used to choose if an input should be added to the corpus or not. In the case
    // below, we're saying that in order for a testcase's input to be added to the corpus it must:
    // 1. be a timeout
    //     AND
    // 2. have created new coverage of the binary under test
    //
    // The goal is to do similar deduplication to what AFL does
    //
    // The feedback and fast macro combines the two feedbacks with a fast AND operation, which
    // means only enough feedback functions will be called to know whether or not the objective
    // has been met, i.e. short-circuiting logic.
    let mut objective =
        feedback_and_fast!(TimeoutFeedback::new(), MaxMapFeedback::new(&edges_observer));

    //
    // Component: Monitor
    //
    // MultiMonitor displays cumulative and per-client statistics (used to be named
    // SimpleStats/MultiStats). It uses LLMP for communication between broker/client(s). It
    // displays 2 clients are connected, even when only a single client is active.
    //
    // Further explanation from domenukk: The 0th client is the client that opens a network socket
    // and listens for other clients and potentially brokers. It's still a client from llmp's
    // perspective, so its more or less an implementation details.
    let monitor = SimpleMonitor::new(|s| println!("{s}"));

    //
    // Component: EventManager
    //
    // The event manager handles the various events generated during the fuzzing loop
    // such as the notification of the addition of a new testcase to the corpus. The SimpleEventManager
    // is the simplest event manager available to us.
    let mut mgr: SimpleEventManager<_, _, _> = SimpleEventManager::new(monitor);

    //
    // Component: State
    //
    // Creates a new State, taking ownership of all of the individual components during fuzzing
    //
    // On the initial pass, setup_restarting_mgr returns (None, LlmpRestartingEventManager).
    // On each successive execution (i.e. on a fuzzer restart), it returns the state from the prior
    // run that was saved off in shared memory. The code below handles the initial None value
    // by providing a default StdState. After the first restart, we'll simply wrap the Some(StdState)
    // returned from the call to setup_restarting_mgr_std
    let mut state = StdState::new(
        // random number generator with a time-based seed
        StdRand::with_seed(current_nanos()),
        input_corpus,
        timeouts_corpus,
        // States of the feedbacks that store the data related to the feedbacks that should be
        // persisted in the state.
        &mut feedback,
        &mut objective,
    );

    //
    // Component: Scheduler
    //
    // A minimization + queue policy to get test cases from the corpus
    //
    // IndexesLenTimeMinimizerCorpusScheduler is a MinimizerCorpusSchedular with a
    // LenTimeMulFacFactor that prioritizes quick and small testcases that excersize all the
    // entries registered in the MapIndexesMetadata
    //
    // a QueueCorpusScheduler walks the corpus ina queue like fashion
    let scheduler = IndexesLenTimeMinimizerScheduler::new(&edges_observer, QueueScheduler::new());

    //
    // Component: Fuzzer
    //
    // A fuzzer with feedback, objectives and a corpus scheduler
    let mut fuzzer = StdFuzzer::new(scheduler, feedback, objective);

    //
    // Component: Executor
    //
    // Creates an in-process executor. The timeoutExecutor wraps the InProcessExecutor and sets a
    // timeout before each run. This gives us an executor that will execute a bunch of testcases
    // within the same process, eliminating a lot of the overhead associated with fork/exec or
    // forkserver execution model.
    let mut executor = ForkserverExecutor::builder()
        .program("./xpdf/install/bin/pdftotext")
        .timeout(Duration::from_secs(5))
        .parse_afl_cmdline(["@@"])
        .coverage_map_size(MAP_SIZE)
        .build(tuple_list!(time_observer, edges_observer))?;

    //
    // Component: Mutator
    //
    // Setup a mutational stage with a basic bytes mutator
    let mutator = StdScheduledMutator::new(havoc_mutations());
    //
    // Component: Stage
    //
    let mut stages = tuple_list!(StdMutationalStage::new(mutator));

    fuzzer
        .fuzz_loop(&mut stages, &mut executor, &mut state, &mut mgr)
        .expect("Error in the fuzzing loop");
}
