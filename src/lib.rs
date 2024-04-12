mod interface_adapters;

pub fn tidy(fast: bool, dirs: Vec<String>, output: Option<String>) {
    interface_adapters::tidy(fast, dirs, output)
}
