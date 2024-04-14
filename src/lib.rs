use std::io;

pub use interface_adapters::Commands;

mod interface_adapters;

pub fn tidy(command: Commands) -> io::Result<()> {
    interface_adapters::tidy(command)
}
