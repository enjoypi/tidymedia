import { expect, test } from "bun:test";
import {
  COV_NONBUSINESS,
  COV_TEST_SUPPORT,
  checkExcludes,
  checkIgnoreMatches,
  hasCode,
  hasCoverageOff,
  ignoreRegex,
  isProductionFile,
  lcovSources,
  missingSources,
  nextestArgs,
  reportArgs,
  zeroHits,
} from "../../tools/cov.ts";

const always = () => true;

test("ignoreRegex 转义点号并兼容两种分隔符", () => {
  const re = new RegExp(ignoreRegex(["src/a/b_io.rs", "src/c.rs"]));
  expect(ignoreRegex(["src/a.rs"])).toBe("(src[/\\\\]a\\.rs)$");
  expect(re.test("src/a/b_io.rs")).toBe(true);
  expect(re.test("C:\\repo\\src\\a\\b_io.rs")).toBe(true);
  expect(re.test("src/a/b_ioxrs")).toBe(false);
  expect(re.test("src/c.rs.bak")).toBe(false);
});

test("checkExcludes 通过合法清单", () => {
  expect(checkExcludes(["src/a.rs", "src/b.rs"], always)).toBeNull();
});

test("checkExcludes 拒绝重复项", () => {
  expect(checkExcludes(["src/a.rs", "src/a.rs"], always)).toBe("duplicate exclusion entries");
});

test("checkExcludes 拒绝缺失文件", () => {
  expect(checkExcludes(["src/a.rs"], () => false)).toBe("missing excluded file: src/a.rs");
});

test("checkExcludes 拒绝测试体", () => {
  expect(checkExcludes(["src/a_tests.rs"], always)).toBe("test body in exclusion list: src/a_tests.rs");
});

test("checkIgnoreMatches 要求正则命中与清单完全一致", () => {
  const excludes = ["src/b.rs", "src/a.rs"];
  const regex = ignoreRegex(excludes);
  expect(checkIgnoreMatches(regex, ["src/a.rs", "src/b.rs", "src/c.rs"], excludes)).toBe(true);
  expect(checkIgnoreMatches(regex, ["src/a.rs", "src/c.rs"], excludes)).toBe(false);
  expect(checkIgnoreMatches(ignoreRegex(["a.rs"]), ["src/a.rs", "src/xa.rs"], ["a.rs"])).toBe(false);
});

test("isProductionFile 排除测试体与测试 helper", () => {
  expect(isProductionFile("src/a.rs")).toBe(true);
  expect(isProductionFile("src/a_tests.rs")).toBe(false);
  expect(isProductionFile("src/a_test_helpers.rs")).toBe(false);
  expect(isProductionFile("src/a_tests_common.rs")).toBe(true);
});

test("zeroHits 只报告计数为 0 的记录并带上所属文件", () => {
  const lcov = [
    "SF:src/a.rs",
    "DA:1,3",
    "DA:2,0",
    "DA:10,10",
    "BRDA:5,0,0,1",
    "BRDA:5,0,1,0",
    "BRDA:6,0,0,-",
    "end_of_record",
    "SF:src/b.rs",
    "DA:7,0",
    "end_of_record",
  ].join("\r\n");
  expect(zeroHits(lcov, "BRDA")).toEqual(["SF:src/a.rs: BRDA:5,0,1,0"]);
  expect(zeroHits(lcov, "DA")).toEqual(["SF:src/a.rs: DA:2,0", "SF:src/b.rs: DA:7,0"]);
});

test("lcovSources 归一化为相对正斜杠路径", () => {
  const lcov = ["SF:C:\\repo\\src\\a.rs", "SF:C:/repo/src/b.rs", "SF:src/c.rs", "DA:1,1"].join("\n");
  expect([...lcovSources(lcov, "C:/repo")].sort()).toEqual(["src/a.rs", "src/b.rs", "src/c.rs"]);
});

test("missingSources 返回不在 lcov 中的生产文件", () => {
  expect(missingSources(["src/a.rs", "src/b.rs"], new Set(["src/a.rs"]))).toEqual(["src/b.rs"]);
});

test("hasCode 只识别顶层 fn 与 impl", () => {
  expect(hasCode("pub mod a;\npub use a::B;\n")).toBe(false);
  expect(hasCode("use x;\nfn a() {}\n")).toBe(true);
  expect(hasCode("pub fn a() {}")).toBe(true);
  expect(hasCode("pub(crate) fn a() {}")).toBe(true);
  expect(hasCode("impl A {}")).toBe(true);
  expect(hasCode("pub impl A {}")).toBe(true);
  expect(hasCode("    fn nested() {}")).toBe(false);
});

test("hasCoverageOff 识别各种空白写法", () => {
  expect(hasCoverageOff("#[coverage(off)]")).toBe(true);
  expect(hasCoverageOff("#[cfg_attr(x, coverage ( off ))]")).toBe(true);
  expect(hasCoverageOff("#[coverage(on)]")).toBe(false);
});

test("reportArgs 仅在非零忽略模式下传 ignore 正则", () => {
  expect(reportArgs("X", false)).toEqual(["--ignore-filename-regex=X"]);
  expect(reportArgs("X", true)).toEqual([]);
});

test("nextestArgs 仅 Windows 加 --no-rustc-wrapper", () => {
  expect(nextestArgs(true).slice(0, 4)).toEqual(["+nightly", "llvm-cov", "--no-rustc-wrapper", "nextest"]);
  expect(nextestArgs(false).slice(0, 3)).toEqual(["+nightly", "llvm-cov", "nextest"]);
  expect(nextestArgs(false)).toContain("--branch");
});

test("排除清单两组互不重叠且非空", () => {
  expect(COV_NONBUSINESS.length).toBeGreaterThan(0);
  expect(COV_TEST_SUPPORT.length).toBeGreaterThan(0);
  expect(COV_NONBUSINESS.filter((p) => COV_TEST_SUPPORT.includes(p))).toEqual([]);
});
