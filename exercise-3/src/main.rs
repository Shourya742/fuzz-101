mod parser;

use libafl_sugar::ForkserverBytesCoverageSugar;
use libafl::bolts::core_affinity::Cores;

fn main() {
    let parsed_opts = parser::parse_args();
    let cores = Cores::from_cmdline(&parsed_opts.cores).expect("Failed to parse cores");

    ForkserverBytesCoverageSugar::builder()
        .input_dirs(&[parsed_opts.input])
        .output_dir(parsed_opts.output)
        .cores(&cores)
        .program(parsed_opts.target)
        .debug_output(parsed_opts.debug)
        .arguments(&parsed_opts.args)
        .build()
        .run()

}