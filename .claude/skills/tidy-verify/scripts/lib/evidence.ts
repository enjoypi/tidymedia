// Step 5 证据收集纯函数：exiftool 全量 dump 解析 / 路径目录日期暗示 / 文件名
// 日期暗示（strong/weak/coincidental）/ 出厂默认时钟判定 / 证据卡片组装。
// 判定表口径见 references/patterns.md；I/O 在 scripts/collect_evidence.ts。

export interface EvidenceEntry {
  source_path: string;
  actual_bucket: string;
  chosen_source: string;
  exif_exp_bucket: string | null;
  exif_from: string | null;
  exif_make: string | null;
  exif_model: string | null;
  filename_bucket: string | null;
  mismatch: boolean;
  patterns: string[];
}

// 候选集 U = MISMATCH ∪ DIFFER（analyze 汇总同口径）。
export function selectReviewEntries(entries: EvidenceEntry[]): EvidenceEntry[] {
  return entries.filter(
    (e) =>
      e.mismatch ||
      (e.filename_bucket !== null && e.filename_bucket !== e.actual_bucket),
  );
}

// 剥 source 根前缀得相对路径（分隔符统一 /）。剥不掉回退原串。
export function relativePath(sourcePath: string, srcRoot: string): string {
  const norm = sourcePath.replaceAll("\\", "/");
  const root = srcRoot.replaceAll("\\", "/").replace(/\/+$/, "");
  if (norm.startsWith(`${root}/`)) {
    return norm.slice(root.length + 1);
  }
  return norm;
}

// exiftool `-s -G` 输出行 `[Group]          TagName                : value`，
// key 用 `Group:TagName` 区分同名 tag（EXIF:CreateDate vs QuickTime:CreateDate）。
export function parseExiftoolDump(text: string): Map<string, string> {
  const map = new Map<string, string>();
  for (const line of text.split("\n")) {
    const m = line.match(/^\[([^\]]+)\]\s+(\S+)\s+:\s(.*)$/);
    if (m) {
      map.set(`${m[1]}:${m[2]}`, (m[3] ?? "").trim());
    }
  }
  return map;
}

export interface PathHint {
  segment: string;
  bucket: string;
  precision: "year" | "month";
}

// 扫每段父目录：月精度（YYYY[.\-_]MM / YYYY年M月）优先，年精度
// （YYYY年M-M月 跨月 / 单独 YYYY 段）兜底；多段命中取第一个最具体的。
export function pathDateHint(relPath: string): PathHint | null {
  const segments = relPath.split("/").slice(0, -1);
  let yearHit: PathHint | null = null;
  for (const seg of segments) {
    const span = seg.match(/(\d{4})年\d{1,2}-\d{1,2}月/);
    if (span && !yearHit) {
      yearHit = { segment: seg, bucket: span[1] ?? "", precision: "year" };
      continue;
    }
    const zh = seg.match(/(\d{4})年(0?[1-9]|1[0-2])月/);
    if (zh) {
      return { segment: seg, bucket: `${zh[1]}:${pad2(zh[2])}`, precision: "month" };
    }
    const num = seg.match(/(\d{4})[.\-_](0?[1-9]|1[0-2])(?!\d)/);
    if (num) {
      return { segment: seg, bucket: `${num[1]}:${pad2(num[2])}`, precision: "month" };
    }
    if (/^\d{4}$/.test(seg) && !yearHit) {
      yearHit = { segment: seg, bucket: seg, precision: "year" };
    }
  }
  return yearHit;
}

export type NameHint = "strong" | "weak" | "coincidental";

// 文件名日期精度：strong = 含合法到秒时间（YYYY-MM-DD HH-MM-SS 类分隔变体）；
// weak = 含合法 YYYYMMDD 但无 HHMMSS；8 位连号非合法日期 = coincidental（巧合 ID）。
export function filenameHintKind(stem: string): NameHint | null {
  const dt = stem.match(
    /(\d{4})-(\d{2})-(\d{2})[ _T](\d{2})[-:](\d{2})[-:](\d{2})/,
  );
  if (dt && validDate(dt[1], dt[2], dt[3]) && validTime(dt[4], dt[5], dt[6])) {
    return "strong";
  }
  const compact = stem.match(/(\d{4})(\d{2})(\d{2})_(\d{2})(\d{2})(\d{2})/);
  if (
    compact &&
    validDate(compact[1], compact[2], compact[3]) &&
    validTime(compact[4], compact[5], compact[6])
  ) {
    return "strong";
  }
  const digits = stem.match(/\d{8}/);
  if (!digits) {
    return null;
  }
  const d = digits[0];
  return validDate(d.slice(0, 4), d.slice(4, 6), d.slice(6, 8))
    ? "weak"
    : "coincidental";
}

// 出厂默认时钟：EXIF 三时间相同且形如 YYYY:01:01 00:00:00。
export function isDefaultClockValue(
  dto: string | undefined,
  create: string | undefined,
  modify: string | undefined,
): boolean {
  if (!dto || dto !== create || dto !== modify) {
    return false;
  }
  return /^\d{4}:01:01 00:00:00$/.test(dto);
}

// 证据卡片：除「推荐」外全字段填充；「推荐」由 AI 研判后补。
export function buildCard(
  entry: EvidenceEntry,
  dump: Map<string, string>,
  relPath: string,
): string {
  const get = (tag: string): string => dump.get(tag) ?? "<无>";
  const stem = (relPath.split("/").pop() ?? "").replace(/\.[^.]*$/, "");
  const pathHint = pathDateHint(relPath);
  const nameKind = filenameHintKind(stem);
  const diagnostics = [...entry.patterns];
  if (pathHint) {
    diagnostics.push("PathDirectoryHint");
  }
  if (nameKind === "strong") {
    diagnostics.push("FilenameStrong");
  } else if (nameKind === "weak") {
    diagnostics.push("FilenameWeakDate");
  } else if (nameKind === "coincidental") {
    diagnostics.push("FilenameCoincidentalDigits");
  }
  if (
    isDefaultClockValue(
      dump.get("EXIF:DateTimeOriginal"),
      dump.get("EXIF:CreateDate"),
      dump.get("EXIF:ModifyDate"),
    )
  ) {
    diagnostics.push("DefaultClockValue");
  }

  const nameHint = entry.filename_bucket
    ? `name=${entry.filename_bucket}${nameKind ? ` (${nameKind})` : ""}`
    : nameKind === "coincidental"
      ? "coincidental"
      : "无";
  const make = dump.get("EXIF:Make") ?? entry.exif_make ?? "<无>";
  const model = dump.get("EXIF:Model") ?? entry.exif_model ?? "<无>";

  return [
    `### ${relPath}`,
    `- **EXIF 时间**: DTO=${get("EXIF:DateTimeOriginal")}, CreateDate=${get("EXIF:CreateDate")}, ModifyDate=${get("EXIF:ModifyDate")}`,
    `- **容器时间**: QT:CreationDate=${get("QuickTime:CreationDate")}, QT:CreateDate=${get("QuickTime:CreateDate")}, Matroska:DateUTC=${get("Matroska:DateUTC")}`,
    `- **文件系统**: mtime=${get("File:FileModifyDate")}, FileCreateDate=${get("File:FileCreateDate")}`,
    `- **相机**: Make=${make}, Model=${model}`,
    `- **路径暗示**: ${pathHint ? `${pathHint.segment} → ${pathHint.bucket} (${pathHint.precision})` : "无"}`,
    `- **文件名暗示**: ${nameHint}`,
    `- **tidymedia 桶**: ${entry.actual_bucket} (from=${entry.chosen_source})`,
    `- **exiftool 桶**: ${entry.exif_exp_bucket ?? "NONE"}`,
    `- **诊断**: ${diagnostics.length > 0 ? diagnostics.map((d) => `\`${d}\``).join(" + ") : "(无)"}`,
    `- **推荐**: (待研判)`,
    "",
  ].join("\n");
}

function pad2(v: string | undefined): string {
  return (v ?? "").padStart(2, "0");
}

function validDate(y: string | undefined, m: string | undefined, d: string | undefined): boolean {
  const mm = Number(m);
  const dd = Number(d);
  return Number(y) >= 1900 && mm >= 1 && mm <= 12 && dd >= 1 && dd <= 31;
}

function validTime(h: string | undefined, m: string | undefined, s: string | undefined): boolean {
  return Number(h) <= 23 && Number(m) <= 59 && Number(s) <= 60;
}
