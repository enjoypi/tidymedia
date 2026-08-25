// 模板改名：把仓库内全部模板 crate 名替换为新项目名（clone 后必改清单第 1 项）。
//
// 用法：
//   bun tools/rename.ts <new_crate_name>
//
// 替换后自动跑 cargo check 验证；CLAUDE.md 中的叙述性文字请人工复核。

import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// 模板名拼接构造，避免本脚本自身被下一次改名误替换。
const TEMPLATE_NAME = "skel" + "_rs";
const SKIP_DIRS = new Set([".git", "target", "node_modules"]);
const SKIP_FILES = new Set(["Cargo.lock"]);

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const selfPath = resolve(fileURLToPath(import.meta.url));

function iterTextFiles(dir: string, out: string[]): void {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name)) iterTextFiles(p, out);
      continue;
    }
    if (SKIP_FILES.has(entry.name) || resolve(p) === selfPath) continue;
    out.push(p);
  }
}

function dirname(p: string): string {
  return resolve(p, "..");
}

const newName = process.argv[2];
if (!newName || process.argv[3]) {
  console.error("用法：bun tools/rename.ts <new_crate_name>");
  process.exit(2);
}
if (!/^[a-z][a-z0-9_]*$/.test(newName)) {
  console.error(`cannot rename: ${newName} 不是合法 crate 名（^[a-z][a-z0-9_]*$）`);
  process.exit(2);
}
if (newName === TEMPLATE_NAME) {
  console.error("cannot rename: 新名与模板名相同");
  process.exit(2);
}

const files: string[] = [];
iterTextFiles(projectRoot, files);
const changed: [string, number][] = [];
for (const p of files) {
  let text: string;
  try {
    text = readFileSync(p, "utf8");
  } catch {
    continue;
  }
  if (!text.includes(TEMPLATE_NAME)) continue;
  const count = text.split(TEMPLATE_NAME).length - 1;
  writeFileSync(p, text.replaceAll(TEMPLATE_NAME, newName));
  changed.push([relative(projectRoot, p), count]);
}

if (changed.length === 0) {
  console.log(`未发现 ${TEMPLATE_NAME}，无需替换（可能已改名）`);
  process.exit(0);
}

for (const [rel, count] of changed.sort(([a], [b]) => (a < b ? -1 : 1))) {
  console.log(`  ${rel}: ${count} 处`);
}
console.log(
  `共 ${changed.length} 个文件替换为 ${newName}，运行 cargo check 验证…`,
);

const check = spawnSync(
  "cargo",
  ["check", "--release", "--workspace", "--features", "http,sqlite"],
  { cwd: projectRoot, stdio: "inherit" },
);
if (check.status !== 0) {
  console.error("cargo check 失败，请检查上方输出");
  process.exit(1);
}
console.log("完成。后续步骤见 CLAUDE.md「clone 后必改清单」2-7 项。");
