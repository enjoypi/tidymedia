import { describe, expect, test } from "bun:test";
import {
  buildCard,
  filenameHintKind,
  isDefaultClockValue,
  parseExiftoolDump,
  pathDateHint,
  relativePath,
  selectReviewEntries,
  type EvidenceEntry,
} from "./evidence.ts";

function entry(over: Partial<EvidenceEntry>): EvidenceEntry {
  return {
    source_path: "/src/a.jpg",
    actual_bucket: "2024:01",
    chosen_source: "FsMtime",
    exif_exp_bucket: null,
    exif_from: null,
    exif_make: null,
    exif_model: null,
    filename_bucket: null,
    mismatch: false,
    patterns: [],
    ...over,
  };
}

describe("selectReviewEntries", () => {
  test("U = mismatch ∪ filename differ", () => {
    const entries = [
      entry({ source_path: "a", mismatch: true }),
      entry({ source_path: "b", filename_bucket: "2019:05" }),
      entry({ source_path: "c", filename_bucket: "2024:01" }),
      entry({ source_path: "d" }),
    ];
    const picked = selectReviewEntries(entries).map((e) => e.source_path);
    expect(picked).toEqual(["a", "b"]);
  });
});

describe("relativePath", () => {
  test("strips root with separators normalized", () => {
    expect(relativePath("D:\\src\\sub\\a.jpg", "D:\\src")).toBe("sub/a.jpg");
    expect(relativePath("/src/sub/a.jpg", "/src/")).toBe("sub/a.jpg");
  });
  test("falls back to full path when root not prefix", () => {
    expect(relativePath("/other/a.jpg", "/src")).toBe("/other/a.jpg");
  });
});

describe("parseExiftoolDump", () => {
  test("parses grouped lines and skips noise", () => {
    const dump = parseExiftoolDump(
      "[EXIF]          DateTimeOriginal            : 2023:12:01 10:00:00\n" +
        "[QuickTime]     CreateDate                  : 2023:12:02 00:00:00\n" +
        "[EXIF]          CreateDate                  : 2023:12:01 10:00:01\n" +
        "not a tag line\n",
    );
    expect(dump.get("EXIF:DateTimeOriginal")).toBe("2023:12:01 10:00:00");
    expect(dump.get("QuickTime:CreateDate")).toBe("2023:12:02 00:00:00");
    expect(dump.get("EXIF:CreateDate")).toBe("2023:12:01 10:00:01");
    expect(dump.size).toBe(3);
  });
});

describe("pathDateHint", () => {
  test("month precision wins over earlier year segment", () => {
    const hint = pathDateHint("2024/2024.03 婚礼/a.jpg");
    expect(hint).toEqual({ segment: "2024.03 婚礼", bucket: "2024:03", precision: "month" });
  });
  test("chinese month form pads month", () => {
    expect(pathDateHint("相册/2024年3月/a.jpg")?.bucket).toBe("2024:03");
  });
  test("spanning months degrade to year precision", () => {
    expect(pathDateHint("2024年3-5月/a.jpg")).toEqual({
      segment: "2024年3-5月",
      bucket: "2024",
      precision: "year",
    });
  });
  test("bare year segment is year precision fallback", () => {
    expect(pathDateHint("photos/2019/a.jpg")).toEqual({
      segment: "2019",
      bucket: "2019",
      precision: "year",
    });
  });
  test("rejects invalid month and no-hit", () => {
    expect(pathDateHint("2024.13/a.jpg")).toBeNull();
    expect(pathDateHint("random/a.jpg")).toBeNull();
  });
});

describe("filenameHintKind", () => {
  test("strong with separated datetime", () => {
    expect(filenameHintKind("xxx 2023-12-01 10-20-30")).toBe("strong");
  });
  test("strong with compact datetime", () => {
    expect(filenameHintKind("IMG_20231201_102030")).toBe("strong");
  });
  test("rejects invalid time then falls through", () => {
    expect(filenameHintKind("2023-12-01 25-00-00")).toBeNull();
  });
  test("weak for valid yyyymmdd without time", () => {
    expect(filenameHintKind("IMG_20231201")).toBe("weak");
  });
  test("coincidental for 8 digits that are not a date", () => {
    expect(filenameHintKind("20231301")).toBe("coincidental");
    expect(filenameHintKind("87654321")).toBe("coincidental");
  });
  test("null without 8-digit run", () => {
    expect(filenameHintKind("IMG_123")).toBeNull();
  });
});

describe("isDefaultClockValue", () => {
  test("three identical jan-first midnights", () => {
    expect(
      isDefaultClockValue(
        "2004:01:01 00:00:00",
        "2004:01:01 00:00:00",
        "2004:01:01 00:00:00",
      ),
    ).toBe(true);
  });
  test("rejects differing values, missing dto, non-default shape", () => {
    expect(
      isDefaultClockValue("2004:01:01 00:00:00", "2004:01:01 00:00:01", "2004:01:01 00:00:00"),
    ).toBe(false);
    expect(isDefaultClockValue(undefined, "2004:01:01 00:00:00", "2004:01:01 00:00:00")).toBe(false);
    expect(
      isDefaultClockValue("2004:02:01 00:00:00", "2004:02:01 00:00:00", "2004:02:01 00:00:00"),
    ).toBe(false);
  });
});

describe("buildCard", () => {
  test("fills every field except recommendation", () => {
    const dump = parseExiftoolDump(
      "[EXIF]          DateTimeOriginal            : 2004:01:01 00:00:00\n" +
        "[EXIF]          CreateDate                  : 2004:01:01 00:00:00\n" +
        "[EXIF]          ModifyDate                  : 2004:01:01 00:00:00\n" +
        "[File]          FileModifyDate              : 2024:01:02 03:04:05\n" +
        "[EXIF]          Make                        : Canon\n",
    );
    const card = buildCard(
      entry({
        source_path: "/src/2024/IMG_20231201.jpg",
        actual_bucket: "2024:01",
        chosen_source: "FsMtime",
        exif_exp_bucket: "2004:01",
        filename_bucket: "2023:12",
        mismatch: true,
        patterns: ["CameraClockUnset"],
      }),
      dump,
      "2024/IMG_20231201.jpg",
    );
    expect(card).toContain("### 2024/IMG_20231201.jpg");
    expect(card).toContain("DTO=2004:01:01 00:00:00");
    expect(card).toContain("mtime=2024:01:02 03:04:05");
    expect(card).toContain("Make=Canon, Model=<无>");
    expect(card).toContain("路径暗示**: 2024 → 2024 (year)");
    expect(card).toContain("文件名暗示**: name=2023:12 (weak)");
    expect(card).toContain("tidymedia 桶**: 2024:01 (from=FsMtime)");
    expect(card).toContain("exiftool 桶**: 2004:01");
    expect(card).toContain("`CameraClockUnset`");
    expect(card).toContain("`PathDirectoryHint`");
    expect(card).toContain("`FilenameWeakDate`");
    expect(card).toContain("`DefaultClockValue`");
    expect(card).toContain("**推荐**: (待研判)");
  });

  test("coincidental stem without filename bucket", () => {
    const card = buildCard(
      entry({ source_path: "/src/87654321.jpg" }),
      new Map(),
      "87654321.jpg",
    );
    expect(card).toContain("文件名暗示**: coincidental");
    expect(card).toContain("`FilenameCoincidentalDigits`");
    expect(card).toContain("exiftool 桶**: NONE");
    expect(card).toContain("路径暗示**: 无");
    expect(card).toContain("DTO=<无>");
  });
});
