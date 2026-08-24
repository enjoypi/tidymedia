//! 9 pattern 诊断引擎（移植 `tidy-verify.md` Step 5.2 的判定表）：基于对账结果
//! 归因可疑文件。当前实现可判定的子集：`TidymediaContainerMiss` /
//! `CameraClockUnset` / `FsTimeIsCopyStamp` / `FilenameDateDiffers` / `ExactDuplicate`；
//! 需机型发布日 DB 的 `ModelReleaseConflict` 与精确日期值的 `DefaultClockValue`
//! 留外部 skill 人工研判。

use crate::entities::media_time::ConflictKind;

/// 判定输入：来自 verify 已算好的各字段。
pub(crate) struct DiagnoseInput<'a> {
    pub actual_bucket: Option<&'a str>,
    pub exp_bucket: Option<&'a str>,
    /// 期望桶来源标签（`QTCreationDate`/`QTCreateDate` 等，见 `exif_tsv`）。
    pub exif_from: Option<&'a str>,
    pub filename_bucket: Option<&'a str>,
    pub conflicts: &'a [ConflictKind],
    pub duplicate_verdict: &'a str,
    pub mismatch: bool,
}

/// 返回命中的 pattern 名列表（顺序固定，供 report 排序稳定）。
#[must_use]
pub(crate) fn patterns(input: &DiagnoseInput<'_>) -> Vec<String> {
    let mut out = Vec::new();
    if input.mismatch
        && input
            .exif_from
            .is_some_and(|f| f == "QTCreationDate" || f == "QTCreateDate")
    {
        out.push("TidymediaContainerMiss".to_owned());
    }
    if input.exp_bucket.is_some_and(|e| e.starts_with("0000:")) {
        out.push("CameraClockUnset".to_owned());
    }
    if input
        .conflicts
        .contains(&ConflictKind::MtimeMuchEarlierThanP0)
    {
        out.push("FsTimeIsCopyStamp".to_owned());
    }
    if let (Some(name), Some(actual)) = (input.filename_bucket, input.actual_bucket)
        && name != actual
    {
        out.push("FilenameDateDiffers".to_owned());
    }
    if input.duplicate_verdict == "exact_dup" {
        out.push("ExactDuplicate".to_owned());
    }
    out
}

/// 修补建议：命中的 pattern 需要人工核对时给 exiftool 命令模板 + 提示；
/// 无可疑或无需修补返 `None`。
#[must_use]
pub(crate) fn fix_suggestion(input: &DiagnoseInput<'_>) -> Option<String> {
    if input.duplicate_verdict == "exact_dup" {
        return None;
    }
    let needs_fix = input.mismatch
        || input.exp_bucket.is_some_and(|e| e.starts_with("0000:"))
        || input
            .filename_bucket
            .zip(input.actual_bucket)
            .is_some_and(|(n, a)| n != a);
    needs_fix.then(|| {
        "人工核对后写回拍摄时间：exiftool -P -overwrite_original \"-AllDates=YYYY:MM:DD \
         HH:MM:SS\" \"-FileModifyDate=YYYY:MM:DD HH:MM:SS\"（视频需显式 \
         -QuickTime:CreateDate= 而非落 XMP），再重跑 verify 收敛"
            .to_owned()
    })
}

#[cfg(test)]
#[path = "diagnose_tests.rs"]
mod tests;
