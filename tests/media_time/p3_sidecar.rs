// spec §2.P3：旁路 sidecar — XMP 与 Google Takeout JSON。

use tidymedia::media_time::sidecar::discover;
use tidymedia::media_time::Source;

/// spec §2.P3：XMP sidecar 中的 photoshop:DateCreated 被识别为 P3 候选。
#[test]
fn xmp_photoshop_datecreated() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("a.jpg");
    std::fs::write(&media, b"jpg").unwrap();
    let xmp = dir.path().join("a.xmp");
    std::fs::write(
        &xmp,
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:Description xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/"
photoshop:DateCreated="2024-05-01T14:30:00+08:00"/>
</x:xmpmeta>"#,
    )
    .unwrap();
    let mp = camino::Utf8PathBuf::from_path_buf(media).unwrap();

    let cands = discover(&mp);
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].source, Source::XmpSidecar);
    // 14:30 +08:00 = 06:30 UTC
    assert_eq!(cands[0].utc.timestamp(), 1_714_545_000);
}

/// spec §2.P3：Google Takeout 的 photoTakenTime.timestamp 被识别为 P3 候选。
#[test]
fn google_takeout_json_phototakentime() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("photo.jpg");
    std::fs::write(&media, b"jpg").unwrap();
    let json = dir.path().join("photo.jpg.json");
    std::fs::write(
        &json,
        r#"{"photoTakenTime":{"timestamp":"1714576200","formatted":"..."}}"#,
    )
    .unwrap();
    let mp = camino::Utf8PathBuf::from_path_buf(media).unwrap();

    let cands = discover(&mp);
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].source, Source::GoogleTakeoutJson);
    assert_eq!(cands[0].utc.timestamp(), 1_714_576_200);
}

/// spec §2.P3：无 sidecar 时返回空。
#[test]
fn no_sidecar_yields_empty() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("alone.jpg");
    std::fs::write(&media, b"jpg").unwrap();
    let mp = camino::Utf8PathBuf::from_path_buf(media).unwrap();
    assert!(discover(&mp).is_empty());
}
