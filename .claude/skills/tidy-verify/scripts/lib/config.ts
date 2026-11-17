/// <reference path="./bun.d.ts" />

import { parseYaml } from "./yaml.ts";

// 平台探测：Windows 上可执行文件带 .exe 后缀；非 Windows 平台仓库里可能同存
// Windows PE 版 .exe（如 bin/exiftool/exiftool.exe），不可执行，MUST NOT 探测。
// 存在性用 BunFile.exists()（size 对缺失文件返回 0，不可作判据）。
export async function resolveBin(base: string): Promise<string | null> {
  const win = typeof process !== "undefined" && process.platform === "win32";
  if (win) {
    if (await exists(`${base}.exe`)) {
      return `${base}.exe`;
    }
    if (await exists(base)) {
      return base;
    }
  } else if (await exists(base)) {
    return base;
  }
  return null;
}

// 系统 PATH 里的可执行文件（如 macOS 经 homebrew 安装的 exiftool）。
function systemBin(name: string): string | null {
  const found = Bun.which(name);
  return found ?? null;
}

async function exists(p: string): Promise<boolean> {
  try {
    return await Bun.file(p).exists();
  } catch {
    return false;
  }
}

export interface SkillConfig {
  workDir: string;
  tidymediaBin: string;
  exiftoolBin: string;
  exifTsv: string;
  exifManifest: string;
  verifyReport: string;
  exiftoolTsvP: string;
  exiftoolConcurrency: number;
  exiftoolMemMb: number;
  exiftoolChunkFiles: number;
  taskMemoryLimitMb: number;
  taskMemoryReserveMb: number;
  fastExtensions: string[];
  incremental: boolean;
  exiftoolPids: string;
  mediaExtensions: string[];
}

function splitList(value: string | undefined): string[] {
  return (value ?? "").split(/\s+/).filter((s) => s !== "");
}

export async function loadConfig(
  configPath = ".claude/skills/tidy-verify/config.yaml",
): Promise<SkillConfig> {
  const raw = parseYaml(await Bun.file(configPath).text());
  return {
    workDir: raw.work_dir ?? "/tmp/tm",
    tidymediaBin: raw.tidymedia_bin ?? "target/release/tidymedia",
    exiftoolBin: raw.exiftool_bin ?? "bin/exiftool/exiftool",
    exifTsv: raw.exif_tsv ?? "exif.tsv",
    exifManifest: raw.exif_manifest ?? "exif.manifest.tsv",
    verifyReport: raw.verify_report ?? "verify.json",
    exiftoolTsvP: (raw.exiftool_tsv_p ?? "").replaceAll("\\t", "\t"),
    exiftoolConcurrency: Number(raw.exiftool_concurrency ?? "16"),
    exiftoolMemMb: Number(raw.exiftool_mem_mb ?? "128"),
    exiftoolChunkFiles: Number(raw.exiftool_chunk_files ?? "512"),
    taskMemoryLimitMb: Number(raw.task_memory_limit_mb ?? "2048"),
    taskMemoryReserveMb: Number(raw.task_memory_reserve_mb ?? "512"),
    fastExtensions: splitList(raw.exiftool_fast_extensions ?? "jpg jpeg png heic heif"),
    incremental: (raw.exiftool_incremental ?? "true") !== "false",
    exiftoolPids: raw.exiftool_pids ?? "exiftool-pids.txt",
    mediaExtensions: splitList(raw.media_extensions),
  };
}

// 返回可执行的 tidymedia 路径；缺失返回 null（调用方应报错退出）。
export async function tidymediaBin(cfg: SkillConfig): Promise<string | null> {
  return resolveBin(cfg.tidymediaBin);
}

// exiftool 探测顺序：repo 内当前平台版本 → 系统 PATH。
// 返回 null 表示本机无 exiftool（macOS 常见），交叉比对跳过。
export async function exiftoolBin(cfg: SkillConfig): Promise<string | null> {
  return (await resolveBin(cfg.exiftoolBin)) ?? systemBin("exiftool");
}

export function workPath(cfg: SkillConfig, name: string): string {
  const sep = cfg.workDir.endsWith("/") || cfg.workDir.endsWith("\\") ? "" : "/";
  return `${cfg.workDir}${sep}${name}`;
}
