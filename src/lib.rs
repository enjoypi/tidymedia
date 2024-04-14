pub use interface_adapters::Commands;

mod interface_adapters;

pub fn tidy(command: Commands) {
    interface_adapters::tidy(command)
}
