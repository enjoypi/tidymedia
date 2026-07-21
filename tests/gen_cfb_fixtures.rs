//! 一次性 CFB fixture 生成器（`#[ignore]`，不进 nextest/覆盖率门槛）。
//!
//! Python `olefile`/`compoundfiles` 只读无法写 CFB，用生产依赖 `cfb` crate 的
//! writer 生成 `tests/data/sample-{doc,xls,ppt}-dated.{doc,xls,ppt}`：
//! 根 storage CLSID 设为各格式官方 GUID（infer crate 靠它 sniff 出
//! msword/ms-excel/ms-powerpoint MIME）+ `\x05SummaryInformation` `PropertySet`
//! （PID_CREATE_DTM=2017-02-14T10:30:00Z → 桶 2017/02）+ 正文 stream
//! （UTF-16LE 中文，供 copy-doc 内容分类的 printable-run 提取）。
//!
//! 手动重生成：`cargo test --release --test gen_cfb_fixtures -- --ignored`
//! （gen.sh 尾注同款命令）。

use std::io::Write;
use std::path::Path;

use uuid::Uuid;

const PID_CREATE_DTM: u32 = 0x0C;
const PID_LASTSAVE_DTM: u32 = 0x0D;
const VT_FILETIME: u32 = 0x40;
const FORMAT_ID_SUMMARY: [u8; 16] = [
    0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3, 0xd9,
];
const FILETIME_TICKS_PER_SEC: u64 = 10_000_000;
const EPOCH_DELTA_SECS: u64 = 11_644_473_600;

/// created=2017-02-14T10:30:00Z / modified=2018-01-01T12:00:00Z（全矩阵统一口径）。
const CREATED_EPOCH: u64 = 1_487_068_200;
const MODIFIED_EPOCH: u64 = 1_514_808_000;

fn unix_to_filetime(unix_secs: u64) -> u64 {
    (unix_secs + EPOCH_DELTA_SECS) * FILETIME_TICKS_PER_SEC
}

// 与 src/entities/office/cfb_tests.rs::build_summary_propertyset 同构（跨 crate
// 边界无法复用测试私有 helper，此处复制并保持字节布局一致）。
fn build_summary_propertyset(created_ft: u64, modified_ft: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0xFE, 0xFF]); // ByteOrder LE
    buf.extend_from_slice(&[0, 0]); // Version
    buf.extend_from_slice(&[0, 0, 0, 0]); // SystemId
    buf.extend_from_slice(&[0u8; 16]); // CLSID
    buf.extend_from_slice(&1u32.to_le_bytes()); // NumPropertySets
    buf.extend_from_slice(&FORMAT_ID_SUMMARY); // FMTID
    buf.extend_from_slice(&48u32.to_le_bytes()); // section offset

    let mut section = Vec::new();
    section.extend_from_slice(&48u32.to_le_bytes()); // section size (8+16+24)
    section.extend_from_slice(&2u32.to_le_bytes()); // num properties
    section.extend_from_slice(&PID_CREATE_DTM.to_le_bytes());
    section.extend_from_slice(&24u32.to_le_bytes());
    section.extend_from_slice(&PID_LASTSAVE_DTM.to_le_bytes());
    section.extend_from_slice(&36u32.to_le_bytes());
    section.extend_from_slice(&VT_FILETIME.to_le_bytes());
    section.extend_from_slice(&created_ft.to_le_bytes());
    section.extend_from_slice(&VT_FILETIME.to_le_bytes());
    section.extend_from_slice(&modified_ft.to_le_bytes());
    buf.extend(section);
    buf
}

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn write_cfb_fixture(
    path: &Path,
    clsid: &str,
    body_stream: &str,
    body_text: &str,
) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut comp = cfb::CompoundFile::create(file)?;
    comp.set_storage_clsid("/", Uuid::parse_str(clsid).expect("valid GUID literal"))?;
    {
        let mut s = comp.create_stream("/\u{5}SummaryInformation")?;
        s.write_all(&build_summary_propertyset(
            unix_to_filetime(CREATED_EPOCH),
            unix_to_filetime(MODIFIED_EPOCH),
        ))?;
    }
    {
        let mut s = comp.create_stream(body_stream)?;
        s.write_all(&utf16le(body_text))?;
    }
    comp.flush()?;
    Ok(())
}

#[test]
#[ignore = "one-off fixture generator; run manually to (re)generate tests/data/sample-{doc,xls,ppt}-dated.*"]
fn generate_cfb_fixtures() {
    let data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    // (文件名, 根 CLSID, 正文 stream 名, 正文文本——供 copy-doc 分类提取)
    let fixtures = [
        (
            "sample-doc-dated.doc",
            "00020906-0000-0000-c000-000000000046", // Word 97-2003
            "/WordDocument",
            "本合同由甲方与乙方签订，双方约定服务条款如下所示。",
        ),
        (
            "sample-xls-dated.xls",
            "00020820-0000-0000-c000-000000000046", // Excel 97-2003
            "/Workbook",
            "增值税发票报销单据金额合计与开票日期明细表。",
        ),
        (
            "sample-ppt-dated.ppt",
            "64818d10-4f9b-11cf-86ea-00aa00b929e8", // PowerPoint 97-2003
            "/PowerPoint Document",
            "项目进展工作报告本季度里程碑与交付总结汇报。",
        ),
    ];
    for (name, clsid, stream, body) in fixtures {
        let path = data_dir.join(name);
        write_cfb_fixture(&path, clsid, stream, body).expect("write CFB fixture");
        println!("wrote {}", path.display());
    }
}
