pub mod use_cases;

pub fn tidy(fast: bool, dirs: Vec<String>, output: Option<String>) {
    use_cases::find_duplicates(fast, dirs, output)
}
