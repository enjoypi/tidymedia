use super::AviExif;
use super::tests_common::avif_with_make;
use super::tests_common::chunk;
use super::tests_common::fixture_bytes;
use super::tests_common::list;
use super::tests_common::parse;
use super::tests_common::riff_avi;
use super::{
    FOURCC_HDRL, FOURCC_LIST, FOURCC_STRD, FOURCC_STRL, MAX_HDRL_BYTES, MAX_LIST_DEPTH,
    MAX_TOP_CHUNKS,
};

// ── happy path：真实 Fujifilm fixture ──

#[test]
fn fixture_extracts_all_four_fields() {
    let got = parse(&fixture_bytes()).expect("fixture must parse");
    assert_eq!(
        got,
        AviExif {
            date_time_original: Some("2005:04:26 20:10:00".into()),
            create_date: Some("2005:04:26 20:10:00".into()),
            make: Some("FUJIFILM".into()),
            model: Some("FinePix E550".into()),
        }
    );
}

// ── find_strd：RIFF 头与顶层扫描 ──

#[test]
fn non_riff_magic_returns_none() {
    assert_eq!(parse(b"JUNK\x00\x00\x00\x00AVI "), None);
}

#[test]
fn riff_but_not_avi_returns_none() {
    assert_eq!(parse(b"RIFF\x04\x00\x00\x00WAVE"), None);
}

#[test]
fn truncated_header_returns_none() {
    assert_eq!(parse(b"RIFF"), None);
}

#[test]
fn truncated_after_header_returns_none() {
    // RIFF 头合法但后续 chunk header 不完整 → read_exact Err。
    let mut bytes = riff_avi(&[]);
    bytes.extend_from_slice(b"LIST"); // 只有 fourcc 没有 size
    assert_eq!(parse(&bytes), None);
}

#[test]
fn skips_non_list_chunk_then_finds_hdrl() {
    // 奇数 size 的 JUNK 钉 padding 逻辑：错 1 字节后 hdrl fourcc 就读歪。
    let strd = avif_with_make(b"FUJIFILM\0");
    let hdrl = list(*FOURCC_HDRL, &chunk(*FOURCC_STRD, &strd));
    let mut chunks = chunk(*b"JUNK", &[0u8; 7]);
    chunks.extend_from_slice(&hdrl);
    let got = parse(&riff_avi(&chunks)).expect("must parse after skipping JUNK");
    assert_eq!(got.make.as_deref(), Some("FUJIFILM"));
}

#[test]
fn skips_non_hdrl_list_then_finds_hdrl() {
    let strd = avif_with_make(b"FUJIFILM\0");
    let mut chunks = list(*b"INFO", &[0u8; 5]); // 非 hdrl LIST（奇数 body 钉 padding）
    chunks.extend_from_slice(&list(*FOURCC_HDRL, &chunk(*FOURCC_STRD, &strd)));
    let got = parse(&riff_avi(&chunks)).expect("must parse after skipping INFO list");
    assert_eq!(got.make.as_deref(), Some("FUJIFILM"));
}

#[test]
fn list_smaller_than_type_field_is_skipped_as_plain_chunk() {
    // size < 4 的 LIST 退化为普通 chunk 跳过（防 size-4 下溢）。
    let strd = avif_with_make(b"FUJIFILM\0");
    let mut chunks = chunk(*FOURCC_LIST, &[0u8; 2]);
    chunks.extend_from_slice(&list(*FOURCC_HDRL, &chunk(*FOURCC_STRD, &strd)));
    assert!(parse(&riff_avi(&chunks)).is_some());
}

#[test]
fn hdrl_over_cap_returns_none() {
    // hdrl 声称 body 超 MAX_HDRL_BYTES → 读取前拒绝。
    let mut bytes = riff_avi(&[]);
    bytes.extend_from_slice(FOURCC_LIST);
    let size = u32::try_from(MAX_HDRL_BYTES + 5).unwrap(); // body = size-4 > cap
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(FOURCC_HDRL);
    assert_eq!(parse(&bytes), None);
}

#[test]
fn top_chunk_budget_exhausted_returns_none() {
    // MAX_TOP_CHUNKS 个非 hdrl chunk 后即使有 hdrl 也放弃。
    let strd = avif_with_make(b"FUJIFILM\0");
    let mut chunks = Vec::new();
    for _ in 0..MAX_TOP_CHUNKS {
        chunks.extend_from_slice(&chunk(*b"JUNK", &[0u8; 2]));
    }
    chunks.extend_from_slice(&list(*FOURCC_HDRL, &chunk(*FOURCC_STRD, &strd)));
    assert_eq!(parse(&riff_avi(&chunks)), None);
}

#[test]
fn hdrl_without_strd_returns_none() {
    let hdrl = list(*FOURCC_HDRL, &chunk(*b"avih", &[0u8; 8]));
    assert_eq!(parse(&riff_avi(&hdrl)), None);
}

// ── find_strd_in：hdrl 内存扫描 ──

#[test]
fn strd_nested_in_strl_is_found() {
    let strd = avif_with_make(b"FUJIFILM\0");
    let stream_list = list(*FOURCC_STRL, &chunk(*FOURCC_STRD, &strd));
    let hdrl = list(*FOURCC_HDRL, &stream_list);
    assert!(parse(&riff_avi(&hdrl)).is_some());
}

#[test]
fn strl_without_strd_falls_through_to_next_chunk() {
    let strd = avif_with_make(b"FUJIFILM\0");
    // 第一个 strl 只有 strh（奇数 body 钉内层 padding），strd 在第二个 strl。
    let mut inner = list(*FOURCC_STRL, &chunk(*b"strh", &[0u8; 3]));
    inner.extend_from_slice(&list(*FOURCC_STRL, &chunk(*FOURCC_STRD, &strd)));
    assert!(parse(&riff_avi(&list(*FOURCC_HDRL, &inner))).is_some());
}

#[test]
fn list_depth_over_cap_returns_none() {
    // hdrl 内再嵌 MAX_LIST_DEPTH+1 层 strl → 超深拒绝。
    let strd = avif_with_make(b"FUJIFILM\0");
    let mut nested = chunk(*FOURCC_STRD, &strd);
    for _ in 0..=MAX_LIST_DEPTH {
        nested = list(*FOURCC_STRL, &nested);
    }
    assert_eq!(parse(&riff_avi(&list(*FOURCC_HDRL, &nested))), None);
}

#[test]
fn strd_size_beyond_buffer_returns_none() {
    // strd 声称 size 越过 hdrl 边界 → buf.get None。
    let mut bad = Vec::new();
    bad.extend_from_slice(FOURCC_STRD);
    bad.extend_from_slice(&100u32.to_le_bytes()); // 实际没有 100 字节
    bad.extend_from_slice(b"AVIF");
    assert_eq!(parse(&riff_avi(&list(*FOURCC_HDRL, &bad))), None);
}

#[test]
fn strl_type_field_beyond_buffer_returns_none() {
    // LIST 声称 size=4 但 body 越界 → buf.get(body..body+4) None。
    let mut bad = Vec::new();
    bad.extend_from_slice(FOURCC_LIST);
    bad.extend_from_slice(&4u32.to_le_bytes());
    bad.push(0); // body 只有 1 字节
    assert_eq!(parse(&riff_avi(&list(*FOURCC_HDRL, &bad))), None);
}

#[test]
fn inner_list_smaller_than_type_field_is_skipped() {
    // hdrl 内 size<4 的 LIST：`size >= 4` 短路为假，按普通 chunk 跳过后仍命中 strd。
    let mut inner = chunk(*FOURCC_LIST, &[0u8; 2]);
    inner.extend_from_slice(&chunk(*FOURCC_STRD, &avif_with_make(b"FUJIFILM\0")));
    assert!(parse(&riff_avi(&list(*FOURCC_HDRL, &inner))).is_some());
}

#[test]
fn inner_non_strl_list_is_skipped() {
    // hdrl 内非 strl 的 LIST（odml 等）：type 可读但不匹配，跳过后命中 strd。
    let mut inner = list(*b"odml", &[0u8; 4]);
    inner.extend_from_slice(&chunk(*FOURCC_STRD, &avif_with_make(b"FUJIFILM\0")));
    assert!(parse(&riff_avi(&list(*FOURCC_HDRL, &inner))).is_some());
}
