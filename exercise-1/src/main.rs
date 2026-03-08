use std::path::PathBuf;

use libafl::{
    corpus::{InMemoryCorpus, OnDiskCorpus},
    events::SimpleEventManager,
    feedback_and_fast, feedback_or,
    feedbacks::{MaxMapFeedback, TimeFeedback, TimeoutFeedback},
    inputs::BytesInput,
    monitors::SimpleMonitor,
    observers::{CanTrack, HitcountsMapObserver, StdMapObserver, TimeObserver},
    state::StdState,
};
use libafl_bolts::{
    current_nanos,
    rands::StdRand,
    shmem::{ShMem, ShMemProvider, StdShMem, StdShMemProvider},
};

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
}
