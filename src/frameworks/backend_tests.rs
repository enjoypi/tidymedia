use camino::Utf8PathBuf;

use super::build_smb_backend;
use crate::entities::common::Error;
use crate::entities::uri::Location;

#[test]
fn build_smb_backend_rejects_non_smb_location() {
    let Err(err) = build_smb_backend(&Location::Local(Utf8PathBuf::from("/tmp/x"))) else {
        panic!("expected error for local location");
    };
    let Error::Io(e) = err;
    assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
    assert!(format!("{e}").contains("requires smb location"), "got: {e}");
}

#[test]
fn build_smb_backend_propagates_config_error() {
    crate::usecases::config::install_loader(|| Err("cfg boom".to_string()));
    let loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from("x"),
    };
    let Err(err) = build_smb_backend(&loc) else {
        panic!("expected config error to propagate");
    };
    let Error::Io(e) = err;
    assert!(format!("{e}").contains("cfg boom"), "got: {e}");
}
