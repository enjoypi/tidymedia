use super::*;

#[test]
fn partial_move_error_is_detected() {
    let e = partial_move_error(
        io::ErrorKind::PermissionDenied,
        "copied a -> b but cannot remove source: denied".to_string(),
    );
    assert!(is_partial_move(&e));
}

#[test]
fn partial_move_error_keeps_kind_and_message() {
    let e = partial_move_error(io::ErrorKind::PermissionDenied, "msg text".to_string());
    assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(e.to_string(), "msg text");
}

#[test]
fn plain_io_error_is_not_partial_move() {
    let e = io::Error::other("cannot remove source lookalike");
    assert!(!is_partial_move(&e));
}

#[test]
fn errorkind_only_error_is_not_partial_move() {
    let e = io::Error::from(io::ErrorKind::NotFound);
    assert!(!is_partial_move(&e));
}
