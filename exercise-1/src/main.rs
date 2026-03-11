use libafl::corpus::{Corpus, InMemoryCorpus, OnDiskCorpus};
use libafl::events::SimpleEventManager;
use libafl::executors::ForkserverExecutor;
use libafl::feedbacks::{MaxMapFeedback, TimeFeedback, TimeoutFeedback};
use libafl::inputs::BytesInput;
use libafl::monitors::SimpleMonitor;
use libafl::mutators::{havoc_mutations, StdScheduledMutator};
use libafl::observers::{CanTrack, HitcountsMapObserver, StdMapObserver, TimeObserver};
use libafl::schedulers::{IndexesLenTimeMinimizerScheduler, QueueScheduler};
use libafl::stages::StdMutationalStage;
use libafl::state::{HasCorpus, StdState};
use libafl::{feedback_and_fast, feedback_or, Error, Fuzzer, StdFuzzer};
use libafl_bolts::rands::StdRand;
use libafl_bolts::shmem::{ShMem, ShMemProvider, StdShMemProvider};
use libafl_bolts::tuples::tuple_list;
use libafl_bolts::{current_nanos, AsSliceMut};
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let corpus_dir = vec![PathBuf::from("./corpus")];

    let input_corpus = InMemoryCorpus::<BytesInput>::new();

    let timeouts_corpus =
        OnDiskCorpus::new(PathBuf::from("./timeouts")).expect("Could not create timeout corpus");

    let time_observer = TimeObserver::new("time");

    const MAP_SIZE: usize = 65536;

    let mut shmem = StdShMemProvider::new()
        .unwrap()
        .new_shmem(MAP_SIZE)
        .unwrap();

    unsafe {
        shmem
            .write_to_env("__AFL_SHM_ID")
            .expect("Couldn't write shared memory ID");
    }

    let mut shmem_map = shmem.as_mut();

    let edges_observer = unsafe {
        HitcountsMapObserver::new(StdMapObserver::new("shared_mem", shmem_map)).track_indices()
    };

    let mut feedback = feedback_or!(
        MaxMapFeedback::new(&edges_observer),
        TimeFeedback::new(&time_observer)
    );

    let mut objective =
        feedback_and_fast!(TimeoutFeedback::new(), MaxMapFeedback::new(&edges_observer));

    let mut state = StdState::new(
        StdRand::with_seed(current_nanos()),
        input_corpus,
        timeouts_corpus,
        &mut feedback,
        &mut objective,
    );

    let monitor = SimpleMonitor::new(|s| println!("{s}"));

    let mut mgr = SimpleEventManager::new(state);

    let scheduler = IndexesLenTimeMinimizerScheduler::new(&edges_observer, QueueScheduler::new());

    let mut fuzzer = StdFuzzer::new(scheduler, feedback, objective);


    let mut executor = ForkserverExecutor::builder()
        .program("./xpdf/install/bin/pdftotext")
        .timeout(Duration::from_secs(5))
        .parse_afl_cmdline(["@@"])
        .coverage_map_size(MAP_SIZE)
        .build(tuple_list!(time_observer, edges_observer))?;

    let mutator = StdScheduledMutator::new(havoc_mutations());
    let mut stages = tuple_list!(StdMutationalStage::new(mutator));

    fuzzer.fuzz_loop(&mut stages, &mut executor, &mut state, &mut mgr).expect("Error in the fuzzing loop");

}
