#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

fn main() -> tidymedia::Result<()> {
    tidymedia::install_config_loader();
    tidymedia::run_cli(std::env::args_os())
}
