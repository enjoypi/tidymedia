//! `entities::riff` 单测：真实 Fujifilm fixture 走 happy path，
//! 合成字节流覆盖 RIFF 扫描 / IFD 解析的全部防御分支。

use std::io::Cursor;

use super::*;

const FIXTURE: &str = "tests/data/sample-fuji-strd.avi";

fn fixture_bytes() -> Vec<u8> {
    std::fs::read(FIXTURE).expect("read AVI fixture")
}

// ── 合成字节流 builder ──

fn chunk(fourcc: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&fourcc);
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0); // RIFF 奇数 size 补齐
    }
    out
}

fn list(list_type: [u8; 4], inner: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&list_type);
    data.extend_from_slice(inner);
    chunk(*FOURCC_LIST, &data)
}

fn riff_avi(chunks: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(FOURCC_AVI);
    data.extend_from_slice(chunks);
    let mut out = Vec::new();
    out.extend_from_slice(FOURCC_RIFF);
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&data);
    out
}

fn ifd_entry(tag: u16, typ: u16, cnt: u32, val: u32) -> Vec<u8> {
    let mut e = Vec::new();
    e.extend_from_slice(&tag.to_le_bytes());
    e.extend_from_slice(&typ.to_le_bytes());
    e.extend_from_slice(&cnt.to_le_bytes());
    e.extend_from_slice(&val.to_le_bytes());
    e
}

/// `AVIF` + 保留区 + 单 entry IFD0（Make 字符串跟在 IFD 之后）。
fn avif_with_make(make: &[u8]) -> Vec<u8> {
    let mut base = Vec::new();
    base.extend_from_slice(&1u16.to_le_bytes()); // count = 1
    // 字符串区 offset：count(2) + entry(12) + next-ifd(4) = 18
    base.extend_from_slice(&ifd_entry(
        TAG_MAKE,
        TYPE_ASCII,
        u32::try_from(make.len()).unwrap(),
        18,
    ));
    base.extend_from_slice(&0u32.to_le_bytes()); // next-IFD 指针
    base.extend_from_slice(make);
    let mut strd = Vec::new();
    strd.extend_from_slice(AVIF_MAGIC);
    strd.extend_from_slice(&[0u8; 4]); // 保留区
    strd.extend_from_slice(&base);
    strd
}

fn parse(bytes: &[u8]) -> Option<AviExif> {
    parse_avi_exif(&mut Cursor::new(bytes.to_vec()))
}

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

// ── parse_avif_ifd：strd 数据解析 ──

#[test]
fn strd_without_avif_magic_returns_none() {
    assert_eq!(parse_avif_ifd(b"TIFF\0\0\0\0\0\0"), None);
}

#[test]
fn strd_shorter_than_magic_returns_none() {
    assert_eq!(parse_avif_ifd(b"AV"), None);
}

#[test]
fn strd_shorter_than_ifd_base_returns_none() {
    assert_eq!(parse_avif_ifd(b"AVIF\0\0"), None);
}

#[test]
fn ifd_count_claims_more_entries_than_present_returns_none() {
    // count=3 但只有 1 个 entry 的空间 → 越界 → 整体拒绝。
    let mut strd = avif_with_make(b"FUJIFILM\0");
    strd[IFD_BASE] = 3;
    assert_eq!(parse_avif_ifd(&strd), None);
}

#[test]
fn missing_exif_offset_still_returns_ifd0_fields() {
    // 无 ExifOffset 指针：第二趟扫描空转，IFD0 的 Make 仍保留。
    let got = parse_avif_ifd(&avif_with_make(b"FUJIFILM\0")).unwrap();
    assert_eq!(got.make.as_deref(), Some("FUJIFILM"));
    assert_eq!(got.date_time_original, None);
}

#[test]
fn inline_ascii_count_le_4_is_rejected() {
    // cnt<=4 的内联 ASCII 对目标标签不应出现，按损坏数据拒绝。
    let mut strd = avif_with_make(b"FUJI\0\0\0\0\0");
    strd[IFD_BASE + 2 + 4] = 4; // entry.cnt: 9 → 4
    let got = parse_avif_ifd(&strd).unwrap();
    assert_eq!(got.make, None);
}

#[test]
fn ascii_count_over_cap_is_rejected() {
    let mut strd = avif_with_make(b"FUJIFILM\0");
    let cnt = u32::try_from(MAX_ASCII_BYTES + 1).unwrap();
    strd[IFD_BASE + 2 + 4..IFD_BASE + 2 + 8].copy_from_slice(&cnt.to_le_bytes());
    assert_eq!(parse_avif_ifd(&strd).unwrap().make, None);
}

#[test]
fn ascii_offset_beyond_buffer_is_rejected() {
    let mut strd = avif_with_make(b"FUJIFILM\0");
    strd[IFD_BASE + 2 + 8..IFD_BASE + 2 + 12].copy_from_slice(&999u32.to_le_bytes());
    assert_eq!(parse_avif_ifd(&strd).unwrap().make, None);
}

#[test]
fn non_utf8_ascii_is_rejected() {
    let got = parse_avif_ifd(&avif_with_make(b"\xff\xfe\xfd\xfc\xfb\0")).unwrap();
    assert_eq!(got.make, None);
}

#[test]
fn blank_ascii_is_rejected() {
    let got = parse_avif_ifd(&avif_with_make(b"    \0\0\0\0\0")).unwrap();
    assert_eq!(got.make, None);
}

#[test]
fn make_with_non_ascii_type_is_ignored() {
    // (TAG_MAKE, 非 TYPE_ASCII) 不匹配任何 arm → 走 `_` 跳过。
    let mut strd = avif_with_make(b"FUJIFILM\0");
    strd[IFD_BASE + 2 + 2] = 1; // entry.typ: 2 → 1 (BYTE)
    assert_eq!(parse_avif_ifd(&strd).unwrap().make, None);
}

#[test]
fn entry_with_unknown_tag_is_skipped() {
    let mut strd = avif_with_make(b"FUJIFILM\0");
    strd[IFD_BASE + 2] = 0xff; // tag 低字节改成未知标签
    let got = parse_avif_ifd(&strd).unwrap();
    assert_eq!(got, AviExif::default());
}

// ── IO 错误注入：截断与 seek 失败 ──

#[test]
fn list_type_truncated_returns_none() {
    // LIST 头完整但 list type 4 字节截断 → read_exact Err。
    let mut bytes = riff_avi(&[]);
    bytes.extend_from_slice(FOURCC_LIST);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.push(b'h'); // type 只有 1 字节
    assert_eq!(parse(&bytes), None);
}

#[test]
fn hdrl_body_truncated_returns_none() {
    // hdrl 声称 100 字节 body 但流只剩 10 字节 → read_exact Err。
    let mut bytes = riff_avi(&[]);
    bytes.extend_from_slice(FOURCC_LIST);
    bytes.extend_from_slice(&104u32.to_le_bytes());
    bytes.extend_from_slice(FOURCC_HDRL);
    bytes.extend_from_slice(&[0u8; 10]);
    assert_eq!(parse(&bytes), None);
}

/// seek 恒 Err 的 reader：钉 `skip` 失败传播（Cursor 的 seek 永不失败，测不到）。
#[derive(Debug)]
struct FailSeek(Cursor<Vec<u8>>);

impl std::io::Read for FailSeek {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.0, buf)
    }
}

impl std::io::Seek for FailSeek {
    fn seek(&mut self, _: std::io::SeekFrom) -> std::io::Result<u64> {
        Err(std::io::Error::other("seek refused"))
    }
}

#[test]
fn seek_failure_on_plain_chunk_returns_none() {
    let mut chunks = chunk(*b"JUNK", &[0u8; 2]);
    chunks.extend_from_slice(&list(
        *FOURCC_HDRL,
        &chunk(*FOURCC_STRD, &avif_with_make(b"FUJIFILM\0")),
    ));
    let mut r = FailSeek(Cursor::new(riff_avi(&chunks)));
    assert_eq!(parse_avi_exif(&mut r), None);
}

#[test]
fn seek_failure_on_non_hdrl_list_returns_none() {
    let mut chunks = list(*b"INFO", &[0u8; 4]);
    chunks.extend_from_slice(&list(
        *FOURCC_HDRL,
        &chunk(*FOURCC_STRD, &avif_with_make(b"FUJIFILM\0")),
    ));
    let mut r = FailSeek(Cursor::new(riff_avi(&chunks)));
    assert_eq!(parse_avi_exif(&mut r), None);
}

#[test]
fn strl_payload_beyond_buffer_returns_none() {
    // strl type 字段在界内但声称的 body 越界 → get(body+4..body+size) None。
    let mut bad = Vec::new();
    bad.extend_from_slice(FOURCC_LIST);
    bad.extend_from_slice(&100u32.to_le_bytes());
    bad.extend_from_slice(FOURCC_STRL); // type 可读，其后不足 96 字节
    bad.extend_from_slice(&[0u8; 8]);
    assert_eq!(parse(&riff_avi(&list(*FOURCC_HDRL, &bad))), None);
}

// ── scan_ifd：entry 各字段处截断 ──

/// `AVIF` + 保留区 + count=1 + entry 的前 `keep` 字节（entry 共 12 字节）。
fn avif_truncated_entry(keep: usize) -> Vec<u8> {
    let mut strd = Vec::new();
    strd.extend_from_slice(AVIF_MAGIC);
    strd.extend_from_slice(&[0u8; 4]);
    strd.extend_from_slice(&1u16.to_le_bytes());
    let entry = ifd_entry(TAG_MAKE, TYPE_ASCII, 9, 18);
    strd.extend_from_slice(&entry[..keep]);
    strd
}

#[test]
fn ifd_entry_truncated_at_typ_returns_none() {
    assert_eq!(parse_avif_ifd(&avif_truncated_entry(2)), None);
}

#[test]
fn ifd_entry_truncated_at_cnt_returns_none() {
    assert_eq!(parse_avif_ifd(&avif_truncated_entry(5)), None);
}

#[test]
fn ifd_entry_truncated_at_val_returns_none() {
    assert_eq!(parse_avif_ifd(&avif_truncated_entry(9)), None);
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
