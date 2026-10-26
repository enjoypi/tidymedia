//! `Location` 的 `Display` / `path` / `with_path` / `ParseError` 行为测试。

use super::*;
use camino::Utf8PathBuf;
use pretty_assertions::assert_eq;

#[test]
fn unsupported_scheme_rejected() {
    let err = Location::parse("ftp://h/p").unwrap_err();
    assert_eq!(err, ParseError::UnsupportedScheme("ftp".into()));
}

#[test]
fn mobile_schemes_rejected() {
    for (uri, scheme) in [
        ("adb://serial/path", "adb"),
        ("mtp://device/storage", "mtp"),
    ] {
        assert_eq!(
            Location::parse(uri),
            Err(ParseError::UnsupportedScheme(scheme.into()))
        );
    }
}

#[test]
fn from_str_dispatches_to_parse() {
    use std::str::FromStr;
    let loc = Location::from_str("/home/me").unwrap();
    assert_eq!(loc, Location::Local(Utf8PathBuf::from("/home/me")));
    assert!(Location::from_str("ftp://x/y").is_err());
}

#[test]
fn scheme_each_variant() {
    assert_eq!(Location::Local(Utf8PathBuf::new()).scheme(), "local");
    assert_eq!(Location::parse("smb://h/s").unwrap().scheme(), "smb");
}

#[test]
fn display_local() {
    assert_eq!(
        Location::Local(Utf8PathBuf::from("/a/b c")).display(),
        "/a/b c"
    );
}

#[test]
fn display_smb_round_trip_full() {
    let original = "smb://al%20ice@nas.local:1445/photos/2024/Jan";
    let loc = Location::parse(original).unwrap();
    let displayed = loc.display();
    let back = Location::parse(&displayed).unwrap();
    assert_eq!(back, loc);
}

#[test]
fn display_smb_no_user_no_port_no_path() {
    let loc = Location::parse("smb://nas/photos").unwrap();
    assert_eq!(loc.display(), "smb://nas/photos");
}

#[test]
fn path_returns_inner_path_each_variant() {
    let local = Location::Local(Utf8PathBuf::from("/a/b/c"));
    assert_eq!(local.path(), Utf8PathBuf::from("/a/b/c"));

    let smb = Location::parse("smb://nas/photos/2024/Jan").unwrap();
    assert_eq!(smb.path(), Utf8PathBuf::from("2024/Jan"));
}

#[test]
fn with_path_preserves_scheme_and_connection_fields() {
    let local = Location::Local(Utf8PathBuf::from("/a"));
    assert_eq!(
        local.with_path(Utf8PathBuf::from("/x/y")),
        Location::Local(Utf8PathBuf::from("/x/y"))
    );

    let smb = Location::parse("smb://alice@nas:1445/photos/old").unwrap();
    let new_smb = smb.with_path(Utf8PathBuf::from("new/sub"));
    assert_eq!(
        new_smb,
        Location::Smb {
            user: Some("alice".into()),
            host: "nas".into(),
            port: Some(1445),
            share: "photos".into(),
            path: Utf8PathBuf::from("new/sub"),
        }
    );
}

#[test]
fn parse_error_display_each_variant() {
    assert!(
        ParseError::MissingHost("x".into())
            .to_string()
            .contains("missing host")
    );
    assert!(
        ParseError::MissingShare("x".into())
            .to_string()
            .contains("missing share")
    );
    assert!(
        ParseError::PercentDecode("x".into())
            .to_string()
            .contains("invalid percent")
    );
    assert!(
        ParseError::UnsupportedScheme("x".into())
            .to_string()
            .contains("unsupported scheme")
    );
    assert!(
        ParseError::InvalidPort("x".into())
            .to_string()
            .contains("invalid port")
    );
}
