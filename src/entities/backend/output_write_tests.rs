use super::*;
use crate::entities::backend::partial_move_error;

#[test]
fn output_write_error_keeps_kind_and_message() {
    let e = output_write_error(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
    assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(e.to_string(), "denied");
}

#[test]
fn wrapped_permission_denied_is_output_write_denied() {
    let e = output_write_error(io::Error::from(io::ErrorKind::PermissionDenied));
    assert!(is_output_write_denied(&e));
}

#[test]
fn wrapped_other_kind_is_not_output_write_denied() {
    let e = output_write_error(io::Error::from(io::ErrorKind::TimedOut));
    assert!(!is_output_write_denied(&e));
}

#[test]
fn plain_permission_denied_is_not_output_write_denied() {
    let e = io::Error::from(io::ErrorKind::PermissionDenied);
    assert!(!is_output_write_denied(&e));
}

#[test]
fn partial_move_permission_denied_is_not_output_write_denied() {
    let e = partial_move_error(
        io::ErrorKind::PermissionDenied,
        "copied a -> b but cannot remove source".into(),
    );
    assert!(!is_output_write_denied(&e));
}
