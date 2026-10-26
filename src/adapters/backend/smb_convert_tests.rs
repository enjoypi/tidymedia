use std::io;
use std::time::SystemTime;

use camino::Utf8PathBuf;

use super::{SmbTarget, entries_from_listing, map_smb_err, metadata_from_file_info, path_or_root};
use crate::entities::backend::{EntryKind, Metadata};
use crate::entities::uri::Location;
use smb2::pack::FileTime;
use smb2::types::Command;
use smb2::types::status::NtStatus;

fn parent(path: &str) -> SmbTarget {
    SmbTarget {
        user: Some("alice".into()),
        host: "nas".into(),
        port: Some(445),
        share: "photos".into(),
        path: Utf8PathBuf::from(path),
        password: Some("parent-pw".into()),
        krb5_ccname: Some("/tmp/parent_cc".into()),
    }
}

fn conn() -> SmbTarget {
    SmbTarget {
        user: Some("bob".into()),
        host: "nas".into(),
        port: Some(445),
        share: "photos".into(),
        path: Utf8PathBuf::from(""),
        password: Some("conn-pw".into()),
        krb5_ccname: Some("/tmp/conn_cc".into()),
    }
}

fn dir_entry(name: &str, size: u64, is_directory: bool) -> smb2::DirectoryEntry {
    smb2::DirectoryEntry {
        name: name.into(),
        size,
        is_directory,
        created: FileTime::ZERO,
        modified: FileTime::ZERO,
    }
}

#[test]
fn path_or_root_maps_empty_to_dot() {
    assert_eq!(path_or_root(&Utf8PathBuf::from("")), ".");
}

#[test]
fn path_or_root_keeps_non_empty_path() {
    assert_eq!(path_or_root(&Utf8PathBuf::from("a/b.jpg")), "a/b.jpg");
}

#[test]
fn map_smb_err_protocol_notfound() {
    let e = smb2::Error::Protocol {
        status: NtStatus::OBJECT_NAME_NOT_FOUND,
        command: Command::Read,
    };
    let mapped = map_smb_err(e);
    assert_eq!(mapped.kind(), io::ErrorKind::NotFound);
    assert!(format!("{mapped}").starts_with("smb2: "), "got: {mapped}");
}

#[test]
fn map_smb_err_protocol_access_denied() {
    let e = smb2::Error::Protocol {
        status: NtStatus::ACCESS_DENIED,
        command: Command::Read,
    };
    assert_eq!(map_smb_err(e).kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn map_smb_err_auth_required() {
    let e = smb2::Error::Auth {
        message: "bad password".into(),
    };
    assert_eq!(map_smb_err(e).kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn map_smb_err_protocol_already_exists() {
    let e = smb2::Error::Protocol {
        status: NtStatus::OBJECT_NAME_COLLISION,
        command: Command::Create,
    };
    assert_eq!(map_smb_err(e).kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn map_smb_err_timeout() {
    assert_eq!(
        map_smb_err(smb2::Error::Timeout).kind(),
        io::ErrorKind::TimedOut
    );
}

#[test]
fn map_smb_err_connection_lost() {
    let e = smb2::Error::Disconnected;
    assert_eq!(map_smb_err(e).kind(), io::ErrorKind::ConnectionAborted);
}

#[test]
fn map_smb_err_session_expired() {
    let e = smb2::Error::SessionExpired;
    assert_eq!(map_smb_err(e).kind(), io::ErrorKind::ConnectionAborted);
}

#[test]
fn map_smb_err_disk_full() {
    let e = smb2::Error::Protocol {
        status: NtStatus::DISK_FULL,
        command: Command::Write,
    };
    assert_eq!(map_smb_err(e).kind(), io::ErrorKind::StorageFull);
}

#[test]
fn map_smb_err_other_kinds_fall_through() {
    let cases = [
        smb2::Error::InvalidData {
            message: "garbled".into(),
        },
        smb2::Error::Cancelled,
        smb2::Error::Io(io::Error::from(io::ErrorKind::BrokenPipe)),
    ];
    for e in cases {
        let rendered = format!("{e}");
        assert_eq!(
            map_smb_err(e).kind(),
            io::ErrorKind::Other,
            "got: {rendered}"
        );
    }
}

#[test]
fn child_target_uses_name_for_empty_parent_path() {
    let child = SmbTarget::child_target(&conn(), &parent(""), "x.jpg");
    assert_eq!(child.path.as_str(), "x.jpg");
}

#[test]
fn child_target_appends_to_nested_parent_path() {
    let child = SmbTarget::child_target(&conn(), &parent("a/b"), "x.jpg");
    assert_eq!(child.path.as_str(), "a/b/x.jpg");
}

#[test]
fn child_target_trims_trailing_slash_on_parent_path() {
    let child = SmbTarget::child_target(&conn(), &parent("a/b/"), "x.jpg");
    assert_eq!(child.path.as_str(), "a/b/x.jpg");
}

#[test]
fn child_target_takes_identity_from_conn_and_credentials_from_parent() {
    let child = SmbTarget::child_target(&conn(), &parent("a"), "x.jpg");
    assert_eq!(child.user.as_deref(), Some("bob"));
    assert_eq!(child.host, "nas");
    assert_eq!(child.port, Some(445));
    assert_eq!(child.share, "photos");
    assert_eq!(child.password.as_deref(), Some("parent-pw"));
    assert_eq!(child.krb5_ccname.as_deref(), Some("/tmp/parent_cc"));
}

#[test]
fn metadata_from_file_info_maps_dir_kind_and_times() {
    let modified = SystemTime::now();
    let created = modified
        .checked_sub(std::time::Duration::from_secs(3600))
        .unwrap();
    let info = smb2::FileInfo {
        size: 0,
        is_directory: true,
        created: FileTime::from_system_time(created),
        modified: FileTime::from_system_time(modified),
        accessed: FileTime::ZERO,
    };
    let meta = metadata_from_file_info(&info);
    assert_eq!(
        meta,
        Metadata {
            size: 0,
            kind: EntryKind::Dir,
            modified: Some(modified),
            created: Some(created),
        }
    );
}

#[test]
fn metadata_from_file_info_maps_file_kind_and_zero_times_to_none() {
    let info = smb2::FileInfo {
        size: 42,
        is_directory: false,
        created: FileTime::ZERO,
        modified: FileTime::ZERO,
        accessed: FileTime::ZERO,
    };
    let meta = metadata_from_file_info(&info);
    assert_eq!(meta.size, 42);
    assert_eq!(meta.kind, EntryKind::File);
    assert_eq!(meta.modified, None);
    assert_eq!(meta.created, None);
}

#[test]
fn entries_from_listing_skips_dot_entries() {
    let entries = entries_from_listing(
        &conn(),
        &parent("dir"),
        vec![
            dir_entry(".", 0, true),
            dir_entry("..", 0, true),
            dir_entry("a.jpg", 3, false),
        ],
    );
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].location.display(),
        "smb://bob@nas:445/photos/dir/a.jpg"
    );
    assert_eq!(entries[0].size, 3);
    assert_eq!(entries[0].kind, EntryKind::File);
}

#[test]
fn entries_from_listing_marks_directories() {
    let entries = entries_from_listing(&conn(), &parent("dir"), vec![dir_entry("sub", 0, true)]);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, EntryKind::Dir);
    let Location::Smb { user, path, .. } = &entries[0].location else {
        panic!("expected smb location");
    };
    assert_eq!(user.as_deref(), Some("bob"));
    assert_eq!(path.as_str(), "dir/sub");
}

#[test]
fn entries_from_listing_uses_conn_identity_for_child_locations() {
    let entries = entries_from_listing(&conn(), &parent(""), vec![dir_entry("root.bin", 7, false)]);
    let Location::Smb { user, path, .. } = &entries[0].location else {
        panic!("expected smb location");
    };
    assert_eq!(user.as_deref(), Some("bob"));
    assert_eq!(path.as_str(), "root.bin");
    assert_eq!(entries[0].size, 7);
}
