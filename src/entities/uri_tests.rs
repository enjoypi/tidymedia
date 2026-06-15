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
    let loc = Location::parse("smb://al%20ice@nas/share/folder%20A/foto%20%E4%B8%AD.jpg").unwrap();
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
    let loc =
        Location::parse("mtp://Pixel%208%20Pro/Internal%20shared%20storage/DCIM/Camera").unwrap();
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

// ── IPv6 bracket host parsing ──
// CLAUDE.md「URI 格式」明确支持 `smb://[::1]/share` 与 `smb://[2001:db8::1]:445/share`
// 形态，但旧测试集无任何 IPv6 用例，让 `split_host_port` 的 `[...]` 分支全部
// branch-uncovered。下述用例锚定 bracket 分支：成功/缺右括号/无端口/有端口/
// 端口前缺冒号/端口非法 6 路径，杀 uri.rs:254-263 整段 BRDA 0 簇。

#[test]
fn smb_ipv6_bracket_no_port() {
    let loc = Location::parse("smb://[::1]/share/path").unwrap();
    let Location::Smb {
        host, port, share, path, ..
    } = loc
    else {
        panic!("expected Smb");
    };
    assert_eq!(host, "[::1]");
    assert!(port.is_none());
    assert_eq!(share, "share");
    assert_eq!(path.as_str(), "path");
}

#[test]
fn smb_ipv6_bracket_with_port() {
    let loc = Location::parse("smb://[2001:db8::1]:445/share").unwrap();
    let Location::Smb {
        host, port, share, ..
    } = loc
    else {
        panic!("expected Smb");
    };
    assert_eq!(host, "[2001:db8::1]");
    assert_eq!(port, Some(445));
    assert_eq!(share, "share");
}

#[test]
fn smb_ipv6_bracket_with_user_and_port() {
    let loc = Location::parse("smb://alice@[::1]:445/share/path").unwrap();
    let Location::Smb {
        user, host, port, ..
    } = loc
    else {
        panic!("expected Smb");
    };
    assert_eq!(user.as_deref(), Some("alice"));
    assert_eq!(host, "[::1]");
    assert_eq!(port, Some(445));
}

// `[` 开头但缺右括号 → `split_host_port` 走 `let Some(end) = find(']') else` 早返
// InvalidPort。覆盖 BRDA:255,0,0(none) + 254,0,0(some) 两端。
#[test]
fn smb_ipv6_bracket_missing_close_bracket_is_invalid_port() {
    assert!(matches!(
        Location::parse("smb://[::1/share"),
        Err(ParseError::InvalidPort(_))
    ));
}

// 右括号后非空且不以 `:` 起头 → strip_prefix(':') = None → InvalidPort，
// 覆盖 BRDA:263,0,0/1。
#[test]
fn smb_ipv6_bracket_trailing_non_colon_is_invalid_port() {
    assert!(matches!(
        Location::parse("smb://[::1]x/share"),
        Err(ParseError::InvalidPort(_))
    ));
}

// 端口位非数字 → `port_str.parse::<u16>()` Err → InvalidPort。
#[test]
fn smb_ipv6_bracket_non_numeric_port_is_invalid_port() {
    assert!(matches!(
        Location::parse("smb://[::1]:abc/share"),
        Err(ParseError::InvalidPort(_))
    ));
}
