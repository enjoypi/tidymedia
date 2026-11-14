import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

export const COV_NONBUSINESS = [
  "src/adapters/backend/local_io.rs",
  "src/adapters/backend/smb_real.rs",
  "src/adapters/classify/tract_embed_real.rs",
  "src/adapters/cli_commands.rs",
  "src/adapters/cli_io.rs",
  "src/adapters/ocr/tract_dbnet_real.rs",
  "src/adapters/report_sink_io.rs",
  "src/adapters/sidecar_io.rs",
  "src/bin/tidymedia.rs",
  "src/entities/common_io.rs",
  "src/entities/exif/exif_io.rs",
  "src/entities/file_info/paths_io.rs",
  "src/entities/m2ts_io.rs",
  "src/entities/office/cfb_io.rs",
  "src/entities/office/flate_io.rs",
  "src/entities/office/plist_io.rs",
  "src/entities/office/read_io.rs",
  "src/entities/office/zip_io.rs",
  "src/entities/png_io.rs",
  "src/entities/riff_io.rs",
  "src/entities/threadpool.rs",
  "src/frameworks/backend.rs",
  "src/frameworks/config_io.rs",
  "src/frameworks/detector.rs",
  "src/usecases/verify/content_diff_io.rs",
  "src/usecases/verify/verify_io.rs",
];

export const COV_TEST_SUPPORT = [
  "src/adapters/backend/fake.rs",
  "src/adapters/backend/fake_ops.rs",
  "src/adapters/backend/fake_writers.rs",
  "src/adapters/classify/fake.rs",
  "src/adapters/classify/tract_embed_tests_common.rs",
  "src/adapters/ocr/fake.rs",
  "src/adapters/ocr/tract_dbnet_tests_common.rs",
  "src/entities/file_index_tests_common.rs",
  "src/entities/file_info/file_info_tests_common.rs",
  "src/entities/office/office_tests_common.rs",
  "src/usecases/copy/copy_tests_common.rs",
  "src/usecases/verify/content_diff_tests_common.rs",
];

const LCOV_OUT = "target/coverage.lcov";
const CODE_RE = /^(impl |pub impl |fn |pub fn |pub\(crate\) fn )/m;
const COVERAGE_OFF_RE = /coverage\s*\(\s*off\s*\)/;

export function ignoreRegex(excludes: string[]): string {
  const parts = excludes.map((p) => p.replaceAll(".", "\\.").replaceAll("/", "[/\\\\]"));
  return `(${parts.join("|")})$`;
}

export function checkExcludes(excludes: string[], exists: (path: string) => boolean): string | null {
  if (new Set(excludes).size !== excludes.length) return "duplicate exclusion entries";
  for (const path of excludes) {
    if (!exists(path)) return `missing excluded file: ${path}`;
    if (path.endsWith("_tests.rs")) return `test body in exclusion list: ${path}`;
  }
  return null;
}

export function checkIgnoreMatches(regex: string, production: string[], excludes: string[]): boolean {
  const re = new RegExp(regex);
  const actual = production.filter((p) => re.test(p)).sort();
  const expected = [...excludes].sort();
  return actual.length === expected.length && actual.every((p, i) => p === expected[i]);
}

export function isProductionFile(path: string): boolean {
  return path.endsWith(".rs") && !path.endsWith("_tests.rs") && !path.endsWith("_test_helpers.rs");
}

export function zeroHits(lcov: string, kind: "BRDA" | "DA"): string[] {
  const hits: string[] = [];
  let source = "";
  for (const line of lcov.split(/\r?\n/)) {
    if (line.startsWith("SF:")) source = line;
    else if (line.startsWith(`${kind}:`) && line.endsWith(",0")) hits.push(`${source}: ${line}`);
  }
  return hits;
}

export function lcovSources(lcov: string, root: string): Set<string> {
  const prefix = `${root}/`;
  const sources = new Set<string>();
  for (const line of lcov.split(/\r?\n/)) {
    if (!line.startsWith("SF:")) continue;
    const path = line.slice(3).replaceAll("\\", "/");
    sources.add(path.startsWith(prefix) ? path.slice(prefix.length) : path);
  }
  return sources;
}

export function missingSources(production: string[], sources: Set<string>): string[] {
  return production.filter((p) => !sources.has(p));
}

export function hasCode(text: string): boolean {
  return CODE_RE.test(text);
}

export function hasCoverageOff(text: string): boolean {
  return COVERAGE_OFF_RE.test(text);
}

export function reportArgs(regex: string, zeroIgnore: boolean): string[] {
  return zeroIgnore ? [] : [`--ignore-filename-regex=${regex}`];
}

export function nextestArgs(windows: boolean): string[] {
  return [
    "+nightly", "llvm-cov", ...(windows ? ["--no-rustc-wrapper"] : []), "nextest",
    "--locked", "--branch", "--cargo-quiet", "--failure-output", "immediate-final", "--no-fail-fast",
    "--no-report", "--show-progress", "none", "--status-level", "fail", "--success-output", "never",
  ];
}

function fail(message: string, details: string[] = []): never {
  if (details.length > 0) console.log(details.join("\n"));
  console.error(`coverage gate failed: ${message}`);
  process.exit(1);
}

function cargo(args: string[]): void {
  const done = Bun.spawnSync(["cargo", ...args], { stdio: ["inherit", "inherit", "inherit"] });
  if (done.exitCode !== 0) process.exit(done.exitCode ?? 1);
}

function sourceFiles(): string[] {
  return [...new Bun.Glob("src/**/*").scanSync({ onlyFiles: true })].map((p) => p.replaceAll("\\", "/")).sort();
}

function main(): void {
  process.chdir(resolve(import.meta.dir, ".."));
  const root = process.cwd().replaceAll("\\", "/");
  const zeroIgnore = process.env.COV_ZERO_IGNORE === "1";
  const excludes = [...COV_NONBUSINESS, ...COV_TEST_SUPPORT];
  const regex = ignoreRegex(excludes);
  const files = sourceFiles();
  const production = files.filter(isProductionFile);

  const problem = checkExcludes(excludes, existsSync);
  if (problem) fail(problem);
  if (!checkIgnoreMatches(regex, production, excludes)) fail("exclusion regex does not match exact file list");

  cargo(["+nightly", "llvm-cov", "clean", "--profraw-only"]);

  const off = files.filter((p) => hasCoverageOff(readFileSync(p, "latin1")));
  if (off.length > 0) fail("coverage(off) is forbidden", off);

  cargo(nextestArgs(process.platform === "win32"));
  const report = ["+nightly", "llvm-cov", "report", ...reportArgs(regex, zeroIgnore)];
  cargo([...report, "--lcov", "--output-path", LCOV_OUT]);
  const lcov = readFileSync(LCOV_OUT, "utf8");

  const branchMiss = zeroHits(lcov, "BRDA");
  if (branchMiss.length > 0) fail("branch BRDA ,0 entries above", branchMiss);

  cargo([
    ...report, "--summary-only", "--fail-under-file-lines", "99.999",
    "--fail-under-functions", "100", "--fail-under-lines", "100", "--fail-under-regions", "100",
  ]);

  const lineMiss = zeroHits(lcov, "DA");
  if (lineMiss.length > 0) fail("line DA ,0 entries above", lineMiss);

  const ignored = new RegExp(regex);
  const gated = zeroIgnore ? production : production.filter((p) => !ignored.test(p));
  const missing = missingSources(gated, lcovSources(lcov, root)).filter((p) => hasCode(readFileSync(p, "utf8")));
  if (missing.length > 0) fail("production source not in lcov:", missing);

  console.log("COV_PASS");
}

if (import.meta.main) main();
