// Step 2：exiftool 递归抽 8 列 tsv（verify --exif-tsv 交叉比对数据源），
// 统一 GBK→UTF-8 规范化。exiftool 缺失时退出 2（交叉比对跳过，verify 内部判定仍有效）。
// Usage: bun extract_exif.ts <source_dir> [work_dir]
/// <reference path="./lib/bun.d.ts" />

import { exiftoolBin, loadConfig, workPath } from "./lib/config.ts";
import { decodeExifText } from "./lib/gbk.ts";

async function main(args: string[]): Promise<number> {
  const src = args[0];
  if (!src) {
    console.error("Usage: bun extract_exif.ts <source_dir> [work_dir]");
    return 2;
  }
  const cfg = await loadConfig();
  const work = args[1] ?? cfg.workDir;
  const bin = await exiftoolBin(cfg);
  if (!bin) {
    console.error(
      "exiftool 缺失：本机无可用 exiftool（Windows 用 repo 内 bin/exiftool/exiftool.exe，" +
        "macOS 需自装）。跳过交叉比对，verify 内部判定仍有效。",
    );
    return 2;
  }

  // MUST NOT 加 -fast2：会跳过 QuickTime moov atom 致 QT 时间读不到，
  // 老 QuickTime（pnot 起头 MOV）会被误判桶一致。
  const proc = Bun.spawn([bin, "-r", "-q", "-T", "-p", cfg.exiftoolTsvP, src]);
  const out = new Uint8Array(await new Response(proc.stdout).arrayBuffer());
  const errBytes = new Uint8Array(await new Response(proc.stderr).arrayBuffer());
  const exit = await proc.exited;

  // Windows Perl 对含中文入口路径按 ANSI(GBK) 输出文件名字节；统一 UTF-8，
  // 换行规整为 LF 与下游解析口径一致。
  const decoded = decodeExifText(out).replace(/\r\n/g, "\n");
  await Bun.write(workPath({ ...cfg, workDir: work }, cfg.exifTsv), decoded);
  await Bun.write(workPath({ ...cfg, workDir: work }, `${cfg.exifTsv}.err`), errBytes);

  console.log(`exif_rows=${countLines(decoded)}`);
  console.log(`exif_err_lines=${countLines(new TextDecoder("utf-8").decode(errBytes))}`);
  console.log(`work_dir=${work}`);
  return exit === 0 ? 0 : exit;
}

// wc -l 口径：数换行符个数；空输出为 0。
function countLines(text: string): number {
  if (text === "") {
    return 0;
  }
  return (text.match(/\n/g) ?? []).length;
}

const code = await main(process.argv.slice(2));
process.exit(code);
