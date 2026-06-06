//! Adb URI 解析 + `Display`/`path`/`with_path`/`ParseError` 行为测试（从 `uri_tests.rs` 拆出）。

use super::*;
use camino::Utf8PathBuf;
use pretty_assertions::assert_eq;

#[test]
fn adb_full_with_serial() {
    let loc = Location::parse("adb://EMULATOR5554/sdcard/DCIM").unwrap();
    assert_eq!(
        loc,
        Location::Adb {
            serial: Some("EMULATOR5554".into()),
            path: Utf8PathBuf::from("/sdcard/DCIM"),
        }
    );
}

#[test]
fn adb_autodetect_empty_serial() {
    let loc = Location::parse("adb:///sdcard/DCIM/Camera").unwrap();
    assert_eq!(
        loc,
        Location::Adb {
            serial: None,
            path: Utf8PathBuf::from("/sdcard/DCIM/Camera"),
        }
    );
}

#[test]
fn adb_percent_decode_serial_and_path() {
    let loc =
        Location::parse("adb://Pixel%208/storage/emulated/0/DCIM/foto%20%E4%B8%AD.jpg").unwrap();
    if let Location::Adb { serial, path } = loc {
        assert_eq!(serial, Some("Pixel 8".into()));
        assert_eq!(
            path,
            Utf8PathBuf::from("/storage/emulated/0/DCIM/foto 中.jpg")
        );
    } else {
        panic!("expected Adb variant");
    }
}

#[test]
fn adb_missing_path_no_slash() {
    assert!(matches!(
        Location::parse("adb://serial"),
        Err(ParseError::MissingPath(_))
    ));
}

#[test]
fn adb_missing_path_trailing_slash() {
    assert!(matches!(
        Location::parse("adb://serial/"),
        Err(ParseError::MissingPath(_))
    ));
}

#[test]
fn adb_missing_path_empty_after_scheme() {
    // `adb:///` 形态：serial 与 tail 均空
    assert!(matches!(
        Location::parse("adb:///"),
        Err(ParseError::MissingPath(_))
    ));
}

#[test]
fn adb_percent_decode_invalid_in_serial() {
    assert!(matches!(
        Location::parse("adb://bad%FFserial/p"),
        Err(ParseError::PercentDecode(_))
    ));
}

#[test]
fn adb_percent_decode_invalid_in_path() {
    assert!(matches!(
        Location::parse("adb://s/bad%FFseg"),
        Err(ParseError::PercentDecode(_))
    ));
}

#[test]
fn unsupported_scheme_rejected() {
    let err = Location::parse("ftp://h/p").unwrap_err();
    assert_eq!(err, ParseError::UnsupportedScheme("ftp".into()));
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
    assert_eq!(Location::parse("mtp://d/s").unwrap().scheme(), "mtp");
    assert_eq!(Location::parse("adb://s/p").unwrap().scheme(), "adb");
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
fn display_mtp_round_trip_full() {
    let loc = Location::parse("mtp://Pixel%208/Internal%20shared%20storage/DCIM/Camera").unwrap();
    let displayed = loc.display();
    let back = Location::parse(&displayed).unwrap();
    assert_eq!(back, loc);
}

#[test]
fn display_mtp_no_path() {
    let loc = Location::parse("mtp://Phone/Card").unwrap();
    assert_eq!(loc.display(), "mtp://Phone/Card");
}

#[test]
fn display_adb_round_trip_with_serial() {
    let original = "adb://EMU%205554/sdcard/DCIM/Camera";
    let loc = Location::parse(original).unwrap();
    let displayed = loc.display();
    let back = Location::parse(&displayed).unwrap();
    assert_eq!(back, loc);
}

#[test]
fn display_adb_autodetect_serial() {
    let loc = Location::parse("adb:///sdcard/DCIM").unwrap();
    assert_eq!(loc.display(), "adb:///sdcard/DCIM");
}

#[test]
fn path_returns_inner_path_each_variant() {
    let local = Location::Local(Utf8PathBuf::from("/a/b/c"));
    assert_eq!(local.path(), Utf8PathBuf::from("/a/b/c"));

    let smb = Location::parse("smb://nas/photos/2024/Jan").unwrap();
    assert_eq!(smb.path(), Utf8PathBuf::from("2024/Jan"));

    let mtp = Location::parse("mtp://Phone/Card/DCIM").unwrap();
    assert_eq!(mtp.path(), Utf8PathBuf::from("DCIM"));

    let adb = Location::parse("adb://s/sdcard/DCIM").unwrap();
    assert_eq!(adb.path(), Utf8PathBuf::from("/sdcard/DCIM"));
}

#[test]
fn with_path_preserves_scheme_and_connection_fields() {
    // Local：换 path 仍是 Local
    let local = Location::Local(Utf8PathBuf::from("/a"));
    assert_eq!(
        local.with_path(Utf8PathBuf::from("/x/y")),
        Location::Local(Utf8PathBuf::from("/x/y"))
    );

    // SMB：user/host/port/share 全部保留，仅 path 覆盖
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

    // MTP：device/storage 保留，path 覆盖
    let mtp = Location::parse("mtp://Pixel/Internal/DCIM").unwrap();
    let new_mtp = mtp.with_path(Utf8PathBuf::from("Movies"));
    assert_eq!(
        new_mtp,
        Location::Mtp {
            device: "Pixel".into(),
            storage: "Internal".into(),
            path: Utf8PathBuf::from("Movies"),
        }
    );

    // ADB：serial 保留，path 覆盖
    let adb = Location::parse("adb://Pixel/sdcard/DCIM").unwrap();
    let new_adb = adb.with_path(Utf8PathBuf::from("/sdcard/Movies"));
    assert_eq!(
        new_adb,
        Location::Adb {
            serial: Some("Pixel".into()),
            path: Utf8PathBuf::from("/sdcard/Movies"),
        }
    );

    let adb_auto = Location::parse("adb:///sdcard/DCIM").unwrap();
    let new_auto = adb_auto.with_path(Utf8PathBuf::from("/sdcard/Movies"));
    assert!(matches!(new_auto, Location::Adb { serial: None, .. }));
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
        ParseError::MissingStorage("x".into())
            .to_string()
            .contains("missing storage")
    );
    assert!(
        ParseError::MissingPath("x".into())
            .to_string()
            .contains("missing path")
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
