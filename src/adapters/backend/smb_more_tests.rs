#[test]
fn parent_target_returns_none_for_root_path() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("only.bin"),
        password: None,
        krb5_ccname: None,
    };
    assert!(t.parent().is_none());
}

#[test]
fn parent_target_returns_some_for_nested_path() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("a/b/c.bin"),
        password: None,
        krb5_ccname: None,
    };
    let p = t.parent().unwrap();
    assert_eq!(p.path.as_str(), "a/b");
}

#[test]
fn parent_target_returns_none_when_parent_empty() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("x.bin"),
        password: None,
        krb5_ccname: None,
    };
    assert!(t.parent().is_none());
}

#[test]
fn parent_target_returns_none_when_parent_is_root_slash() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("/foo"),
        password: None,
        krb5_ccname: None,
    };
    assert!(t.parent().is_none(), "/foo 的 parent '/' 被哨兵拒返 None");
}

#[test]
fn build_target_threads_env_password_and_krb5() {
    unsafe {
        std::env::set_var("SMB_PASSWORD", "secret-pw");
        std::env::set_var("KRB5CCNAME", "/tmp/krb5cc_0");
    }
    let t = SmbTarget::from_location(&smb("a.bin"), &()).unwrap();
    assert_eq!(t.password.as_deref(), Some("secret-pw"));
    assert_eq!(t.krb5_ccname.as_deref(), Some("/tmp/krb5cc_0"));
    assert_eq!(t.user.as_deref(), Some("alice"));
    assert_eq!(t.host, "nas");
    assert_eq!(t.port, Some(445));
    assert_eq!(t.share, "photos");
    unsafe {
        std::env::remove_var("SMB_PASSWORD");
        std::env::remove_var("KRB5CCNAME");
    }
}

#[test]
fn build_target_leaves_password_none_when_env_unset() {
    unsafe {
        std::env::remove_var("SMB_PASSWORD");
        std::env::remove_var("KRB5CCNAME");
    }
    let t = SmbTarget::from_location(&smb("a.bin"), &()).unwrap();
    assert!(t.password.is_none());
    assert!(t.krb5_ccname.is_none());
}

#[test]
fn smb_target_equality_and_debug() {
    let t1 = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("x"),
        password: None,
        krb5_ccname: None,
    };
    let t2 = t1.clone();
    assert_eq!(t1, t2);
    let _ = format!("{t1:?}");
}

#[test]
fn parent_target_returns_none_for_empty_path() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from(""),
        password: None,
        krb5_ccname: None,
    };
    assert!(t.parent().is_none());
}

#[test]
fn arc_with_client_builds_dyn_backend() {
    let client: Arc<dyn SmbClient> = fake_client();
    let backend = SmbBackend::arc_with_client(client);
    assert_eq!(backend.scheme(), "smb");
}

#[test]
fn smb_target_entry_location_constructs_uri() {
    let t = SmbTarget {
        user: Some("alice".into()),
        host: "nas".into(),
        port: Some(445),
        share: "photos".into(),
        path: Utf8PathBuf::from("a/b.jpg"),
        password: None,
        krb5_ccname: None,
    };
    let loc = t.entry_location(Utf8PathBuf::from("x/y.jpg"));
    assert_eq!(
        loc,
        Location::Smb {
            user: Some("alice".into()),
            host: "nas".into(),
            port: Some(445),
            share: "photos".into(),
            path: Utf8PathBuf::from("x/y.jpg"),
        }
    );
}

#[test]
fn smb_target_path_returns_inner_path() {
    let t = SmbTarget {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from("a/b.jpg"),
        password: None,
        krb5_ccname: None,
    };
    assert_eq!(t.path().as_str(), "a/b.jpg");
}
