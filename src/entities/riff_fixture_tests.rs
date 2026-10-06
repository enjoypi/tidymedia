use std::io::Cursor;

pub(super) use super::*;

// 测试本地常量（与 tiff_ifd / EXIF 规范一致）。
pub(super) const TAG_MAKE: u16 = 0x010f;
pub(super) const TYPE_ASCII: u16 = 2;
pub(super) const MAX_ASCII_BYTES: usize = 256;

pub(super) const FIXTURE: &str = "tests/data/sample-fuji-strd.avi";

pub(super) fn fixture_bytes() -> Vec<u8> {
    std::fs::read(FIXTURE).expect("read AVI fixture")
}

pub(super) fn chunk(fourcc: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&fourcc);
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0); // RIFF 奇数 size 补齐
    }
    out
}

pub(super) fn list(list_type: [u8; 4], inner: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&list_type);
    data.extend_from_slice(inner);
    chunk(*FOURCC_LIST, &data)
}

pub(super) fn riff_avi(chunks: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(FOURCC_AVI);
    data.extend_from_slice(chunks);
    let mut out = Vec::new();
    out.extend_from_slice(FOURCC_RIFF);
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&data);
    out
}

pub(super) fn ifd_entry(tag: u16, typ: u16, cnt: u32, val: u32) -> Vec<u8> {
    let mut e = Vec::new();
    e.extend_from_slice(&tag.to_le_bytes());
    e.extend_from_slice(&typ.to_le_bytes());
    e.extend_from_slice(&cnt.to_le_bytes());
    e.extend_from_slice(&val.to_le_bytes());
    e
}

/// `AVIF` + 保留区 + 单 entry IFD0（Make 字符串跟在 IFD 之后）。
pub(super) fn avif_with_make(make: &[u8]) -> Vec<u8> {
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

pub(super) fn parse(bytes: &[u8]) -> Option<AviExif> {
    parse_avi_exif(&mut Cursor::new(bytes.to_vec()))
}

/// `AVIF` + 保留区 + count=1 + entry 的前 `keep` 字节（entry 共 12 字节）。
pub(super) fn avif_truncated_entry(keep: usize) -> Vec<u8> {
    let mut strd = Vec::new();
    strd.extend_from_slice(AVIF_MAGIC);
    strd.extend_from_slice(&[0u8; 4]);
    strd.extend_from_slice(&1u16.to_le_bytes());
    let entry = ifd_entry(TAG_MAKE, TYPE_ASCII, 9, 18);
    strd.extend_from_slice(&entry[..keep]);
    strd
}

/// seek 恒 Err 的 reader：钉 `skip` 失败传播（Cursor 的 seek 永不失败，测不到）。
#[derive(Debug)]
pub(super) struct FailSeek(pub(super) Cursor<Vec<u8>>);

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
