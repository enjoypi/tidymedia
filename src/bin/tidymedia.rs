#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

fn main() -> tidymedia::Result<()> {
    tidymedia::run_cli(std::env::args_os())
}
