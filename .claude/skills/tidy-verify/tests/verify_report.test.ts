/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { analyzeVerifyReport } from "../scripts/lib/verify_report.ts";

const fixture = {
  scanned: 5,
  compared: 5,
  mismatched: 2,
  decision_failed: 0,
  pattern_counts: {
    TidymediaContainerMiss: 1,
    ExactDuplicate: 1,
    FilenameDateDiffers: 1,
  },
  entries: [
    {
      source_path: "D:/Pics/2010/IMG_0001.jpg",
      actual_bucket: "2010:06",
      exif_exp_bucket: "2010:06",
      exif_from: "DTO",
      exif_make: "Canon",
      exif_model: "Canon EOS 5D",
      filename_bucket: "2010:06",
      mismatch: false,
      duplicate_verdict: "absent",
      patterns: [],
    },
    {
      source_path: "D:/Pics/老片/IMG_0002.jpg",
      actual_bucket: "2007:01",
      exif_exp_bucket: "2006:08",
      exif_from: "QTCreationDate",
      exif_make: "Sony",
      exif_model: "DSC-H1",
      filename_bucket: "2007:01",
      mismatch: true,
      duplicate_verdict: "absent",
      patterns: ["TidymediaContainerMiss"],
    },
    {
      source_path: "D:/Pics/2008/IMG_0003.jpg",
      actual_bucket: "2008:02",
      exif_exp_bucket: "2008:10",
      exif_from: "DTO",
      exif_make: "Nikon",
      exif_model: "D90",
      filename_bucket: "2008:10",
      mismatch: true,
      duplicate_verdict: "absent",
      patterns: ["FilenameDateDiffers"],
    },
    {
      source_path: "D:/Pics/2009/IMG_0004.jpg",
      actual_bucket: "2009:05",
      exif_exp_bucket: "2009:05",
      exif_from: "DTO",
      filename_bucket: "2009:05",
      mismatch: false,
      duplicate_verdict: "exact_dup",
      patterns: ["ExactDuplicate"],
    },
    {
      source_path: "D:/Pics/2010/IMG_0005.jpg",
      actual_bucket: "2010:01",
      exif_exp_bucket: "2010:01",
      exif_from: "DTO",
      filename_bucket: null,
      mismatch: false,
      duplicate_verdict: "pixel_same",
      patterns: [],
    },
  ],
};

test("汇总 MISMATCH 与 from 标签", () => {
  const s = analyzeVerifyReport(fixture);
  expect(s.scanned).toBe(5);
  expect(s.compared).toBe(5);
  expect(s.mismatched).toBe(2);
  expect(s.mismatchRows.length).toBe(2);
  expect(s.mismatchRows[0].exp).toBe("2006:08");
  expect(s.mismatchRows[0].from).toBe("QTCreationDate");
  expect(s.mismatchRows[0].make).toBe("Sony");
  expect(s.mismatchRows[1].tgt).toBe("2008:02");
});

test("DIFFER 仅文件名桶与预测桶不一致者", () => {
  const s = analyzeVerifyReport(fixture);
  expect(s.differRows.length).toBe(1);
  expect(s.differRows[0].name).toBe("2008:10");
  expect(s.differRows[0].source).toContain("IMG_0003");
});

test("duplicate_verdict 分布与 with_name_time", () => {
  const s = analyzeVerifyReport(fixture);
  expect(s.verdictCounts.absent).toBe(3);
  expect(s.verdictCounts.exact_dup).toBe(1);
  expect(s.verdictCounts.pixel_same).toBe(1);
  expect(s.with_name_time).toBe(4);
});

test("pattern_counts 直接采用 verify 汇总不重复累加", () => {
  const s = analyzeVerifyReport(fixture);
  expect(s.patternCounts.TidymediaContainerMiss).toBe(1);
  expect(s.patternCounts.FilenameDateDiffers).toBe(1);
});

test("空 entries 返回空汇总", () => {
  const s = analyzeVerifyReport({ scanned: 0, compared: 0, entries: [] });
  expect(s.mismatchRows).toEqual([]);
  expect(s.differRows).toEqual([]);
  expect(s.with_name_time).toBe(0);
  expect(s.decision_failed).toBe(0);
});

test("字段缺失容错（undefined 字段按缺省处理）", () => {
  const s = analyzeVerifyReport({
    scanned: 1,
    entries: [{ source_path: "x.jpg", actual_bucket: "2024:05" }],
  });
  expect(s.mismatchRows.length).toBe(0);
  expect(s.verdictCounts.not_checked).toBe(1);
  expect(s.with_name_time).toBe(0);
});
