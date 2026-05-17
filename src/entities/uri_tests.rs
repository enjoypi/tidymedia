use super::*;
use camino::Utf8PathBuf;
use pretty_assertions::assert_eq;

#[test]
fn local_absolute_path() {
    assert_eq!(
        Location::parse("/abs/dir").unwrap(),
        Location::Local(Utf8PathBuf::from("/abs/dir"))
    );
}

#[test]
fn local_relative_path() {
    assert_eq!(
        Location::parse("./rel/dir").unwrap(),
        Location::Local(Utf8PathBuf::from("./rel/dir"))
    );
}

#[test]
fn local_windows_drive_letter_not_scheme() {
    // 含 `:` 但不含 `://`，按本地路径解析
    assert_eq!(
        Location::parse("C:\\Users\\me").unwrap(),
        Location::Local(Utf8PathBuf::from("C:\\Users\\me"))
    );
}

#[test]
fn local_explicit_scheme() {
    assert_eq!(
        Location::parse("local:///abs/path%20with%20space").unwrap(),
        Location::Local(Utf8PathBuf::from("/abs/path with space"))
    );
}

#[test]
fn local_explicit_scheme_percent_decode_error() {
    let err = Location::parse("local:///bad%FFutf8").unwrap_err();
    assert!(matches!(err, ParseError::PercentDecode(_)));
}

#[test]
fn smb_full_with_user_and_port() {
    let loc = Location::parse("smb://alice@nas.local:1445/photos/2024/Jan").unwrap();
    assert_eq!(
        loc,
        Location::Smb {
            user: Some("alice".into()),
            host: "nas.local".into(),
            port: Some(1445),
            share: "photos".into(),
            path: Utf8PathBuf::from("2024/Jan"),
        }
    );
}

#[test]
fn smb_minimal_share_no_path() {
    let loc = Location::parse("smb://nas/photos").unwrap();
    assert_eq!(
        loc,
        Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "photos".into(),
            path: Utf8PathBuf::new(),
        }
    );
}

#[test]
fn smb_percent_decode_user_and_path() {
    let loc = Location::parse("smb://al%20ice@nas/share/folder%20A/foto%20%E4%B8%AD.jpg")
        .unwrap();
    if let Location::Smb {
        user, share, path, ..
    } = loc
    {
        assert_eq!(user, Some("al ice".into()));
        assert_eq!(share, "share");
        assert_eq!(path, Utf8PathBuf::from("folder A/foto 中.jpg"));
    } else {
        panic!("expected Smb variant");
    }
}

#[test]
fn smb_missing_share_no_slash() {
    assert!(matches!(
        Location::parse("smb://nas"),
        Err(ParseError::MissingShare(_))
    ));
}

#[test]
fn smb_empty_share_trailing_slash() {
    assert!(matches!(
        Location::parse("smb://nas/"),
        Err(ParseError::MissingShare(_))
    ));
}

#[test]
fn smb_missing_host_empty_auth() {
    assert!(matches!(
        Location::parse("smb:///share/path"),
        Err(ParseError::MissingHost(_))
    ));
}

#[test]
fn smb_invalid_port_non_numeric() {
    assert!(matches!(
        Location::parse("smb://nas:abc/share"),
        Err(ParseError::InvalidPort(_))
    ));
}

#[test]
fn smb_invalid_port_out_of_range() {
    assert!(matches!(
        Location::parse("smb://nas:70000/share"),
        Err(ParseError::InvalidPort(_))
    ));
}

#[test]
fn smb_percent_decode_invalid_in_user() {
    assert!(matches!(
        Location::parse("smb://bad%FFuser@nas/share"),
        Err(ParseError::PercentDecode(_))
    ));
}

#[test]
fn smb_percent_decode_invalid_in_share() {
    assert!(matches!(
        Location::parse("smb://nas/bad%FFshare"),
        Err(ParseError::PercentDecode(_))
    ));
}

#[test]
fn smb_percent_decode_invalid_in_path_segment() {
    assert!(matches!(
        Location::parse("smb://nas/share/ok/bad%FFseg"),
        Err(ParseError::PercentDecode(_))
    ));
}

#[test]
fn smb_percent_decode_invalid_in_share_with_path() {
    // share/storage 段 decode 失败、且后续仍有 path 段：覆盖 split_first_segment
    // 的 Some 分支 first decode Err 路径。
    assert!(matches!(
        Location::parse("smb://nas/bad%FFshare/sub"),
        Err(ParseError::PercentDecode(_))
    ));
}

#[test]
fn mtp_percent_decode_invalid_in_storage_with_path() {
    assert!(matches!(
        Location::parse("mtp://Phone/bad%FFstore/sub"),
        Err(ParseError::PercentDecode(_))
    ));
}

#[test]
fn mtp_full_with_path() {
    let loc = Location::parse(
        "mtp://Pixel%208%20Pro/Internal%20shared%20storage/DCIM/Camera",
    )
    .unwrap();
    assert_eq!(
        loc,
        Location::Mtp {
            device: "Pixel 8 Pro".into(),
            storage: "Internal shared storage".into(),
            path: Utf8PathBuf::from("DCIM/Camera"),
        }
    );
}

#[test]
fn mtp_minimal_storage_only() {
    let loc = Location::parse("mtp://Phone/Card").unwrap();
    assert_eq!(
        loc,
        Location::Mtp {
            device: "Phone".into(),
            storage: "Card".into(),
            path: Utf8PathBuf::new(),
        }
    );
}

#[test]
fn mtp_missing_storage_no_slash() {
    assert!(matches!(
        Location::parse("mtp://Phone"),
        Err(ParseError::MissingStorage(_))
    ));
}

#[test]
fn mtp_empty_storage_trailing_slash() {
    assert!(matches!(
        Location::parse("mtp://Phone/"),
        Err(ParseError::MissingStorage(_))
    ));
}

#[test]
fn mtp_missing_device_empty_prefix() {
    assert!(matches!(
        Location::parse("mtp:///storage/p"),
        Err(ParseError::MissingHost(_))
    ));
}

#[test]
fn mtp_percent_decode_invalid_in_device() {
    assert!(matches!(
        Location::parse("mtp://bad%FFdev/storage"),
        Err(ParseError::PercentDecode(_))
    ));
}

#[test]
fn mtp_percent_decode_invalid_in_storage() {
    assert!(matches!(
        Location::parse("mtp://Phone/bad%FFstore"),
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
    assert_eq!(
        Location::parse("smb://h/s").unwrap().scheme(),
        "smb"
    );
    assert_eq!(
        Location::parse("mtp://d/s").unwrap().scheme(),
        "mtp"
    );
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
    let loc =
        Location::parse("mtp://Pixel%208/Internal%20shared%20storage/DCIM/Camera")
            .unwrap();
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
fn path_returns_inner_path_each_variant() {
    let local = Location::Local(Utf8PathBuf::from("/a/b/c"));
    assert_eq!(local.path(), Utf8PathBuf::from("/a/b/c"));

    let smb = Location::parse("smb://nas/photos/2024/Jan").unwrap();
    assert_eq!(smb.path(), Utf8PathBuf::from("2024/Jan"));

    let mtp = Location::parse("mtp://Phone/Card/DCIM").unwrap();
    assert_eq!(mtp.path(), Utf8PathBuf::from("DCIM"));
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
}

#[test]
fn parse_error_display_each_variant() {
    assert!(ParseError::MissingHost("x".into())
        .to_string()
        .contains("missing host"));
    assert!(ParseError::MissingShare("x".into())
        .to_string()
        .contains("missing share"));
    assert!(ParseError::MissingStorage("x".into())
        .to_string()
        .contains("missing storage"));
    assert!(ParseError::PercentDecode("x".into())
        .to_string()
        .contains("invalid percent"));
    assert!(ParseError::UnsupportedScheme("x".into())
        .to_string()
        .contains("unsupported scheme"));
    assert!(ParseError::InvalidPort("x".into())
        .to_string()
        .contains("invalid port"));
}
