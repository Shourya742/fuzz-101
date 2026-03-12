use std::{path::PathBuf, time::Duration};

use libafl::{
    Error, StdFuzzer,
    corpus::{InMemoryCorpus, OnDiskCorpus},
    events::setup_restarting_mgr_std,
    executors::{ExitKind, InProcessExecutor},
    feedback_and_fast, feedback_or,
    feedbacks::{CrashFeedback, MaxMapFeedback, TimeFeedback},
    inputs::{BytesInput, HasTargetBytes},
    monitors::MultiMonitor,
    mutators::havoc_mutations,
    observers::{CanTrack, HitcountsMapObserver, TimeObserver},
    schedulers::{IndexesLenTimeMinimizerScheduler, QueueScheduler},
    stages::StdMutationalStage,
    state::StdState,
};
use libafl_bolts::{AsSlice, current_nanos, rands::StdRand, tuples::tuple_list};
use libafl_targets::{libfuzzer_test_one_input, std_edges_map_observer};

#[no_mangle]
fn libafl_main() -> Result<(), Error> {
    let corpus_dir = vec![PathBuf::from("./corpus")];
    let input_corpus = InMemoryCorpus::<BytesInput>::new();
    let solution_corpus = OnDiskCorpus::new(PathBuf::from("./solutions")).unwrap();

    let edges_observer =
        HitcountsMapObserver::new(unsafe { std_edges_map_observer("edges") }).track_indices();
    let time_observer = TimeObserver::new("time");

    let mut feedback = feedback_or!(
        MaxMapFeedback::new(&edges_observer),
        TimeFeedback::new(&time_observer)
    );

    let mut objective =
        feedback_and_fast!(CrashFeedback::new(), MaxMapFeedback::new(&edges_observer));

    let monitor = MultiMonitor::new(|s| {
        println!("{}", s);
    });

    let (state, mut mgr) =
        match setup_restarting_mgr_std(monitor, 1337, libafl::events::EventConfig::AlwaysUnique) {
            Ok(res) => res,
            Err(err) => match err {
                Error::ShuttingDown => {
                    return Ok(());
                }
                _ => {
                    panic!("Failed to setup the restarting manager: {err}");
                }
            },
        };

    let mut state = state.unwrap_or_else(|| {
        StdState::new(
            StdRand::with_seed(current_nanos()),
            input_corpus,
            solution_corpus,
            &mut feedback,
            &mut objective,
        )
        .unwrap()
    });

    let scheduler = IndexesLenTimeMinimizerScheduler::new(&edges_observer, QueueScheduler::new());

    let mut fuzzer = StdFuzzer::new(scheduler, feedback, objective);

    let mut harness = |input: &BytesInput| {
        let target = input.target_bytes();
        let buffer = target.as_slice();
        unsafe {
            libfuzzer_test_one_input(&[]);
        }
        ExitKind::Ok
    };

    let mut in_proc_executor = InProcessExecutor::with_timeout(
        &mut harness,
        tuple_list!(edges_observer, time_observer),
        &mut fuzzer,
        &mut state,
        &mut mgr,
        Duration::from_millis(5000),
    )
    .unwrap();

    let mutator = StdScheduledMutator::new(havoc_mutations());
    let mut stages = tuple_list!(StdMutationalStage::new(mutator));

    Ok(())
}
