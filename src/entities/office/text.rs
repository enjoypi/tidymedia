use crate::entities::backend::MediaReader;

use super::read_io;

#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, mime: &str) -> (u64, u64) {
    let _ = reader;
    let _ = mime;
    (0, 0)
}

#[doc(hidden)]
pub fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let Some(buf) = read_io::read_capped(reader, max_bytes) else {
        return String::new();
    };
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
#[path = "text_deep_tests.rs"]
mod deep_tests;

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
