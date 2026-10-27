use super::{Error, key_from_display};

#[test]
fn display_key_normalizes_smb_aliases_and_keeps_unparsed_paths() {
    let a = key_from_display("smb://user@NAS:445/Share/A.JPG");
    let b = key_from_display("smb://nas/share/a.jpg");
    assert_eq!(a, b);
    assert_eq!(key_from_display("bad://path"), "bad://path");
}

#[test]
fn io_error_display_contains_inner_message() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
    let err: Error = io_err.into();
    let msg = format!("{err}");
    assert!(msg.starts_with("IO error occurred:"), "got: {msg}");
    assert!(msg.contains("no such file"), "got: {msg}");
}
