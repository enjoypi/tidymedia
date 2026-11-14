import { appendFileSync, copyFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const SCRIPT_DIR = import.meta.dir;
const DATA_DIR = resolve(SCRIPT_DIR, "..", "data");
const BASE = "sample-with-exif.jpg";
const BLANK = "blank-noexif.jpg";

const EXIF_VARIANTS: [string, string[]][] = [
  ["sample-with-offset.jpg", ["-DateTimeOriginal=2024:05:01 14:30:00", "-OffsetTimeOriginal=+08:00", "-CreateDate=", "-ModifyDate="]],
  ["sample-modifydate-only.jpg", ["-DateTimeOriginal=", "-CreateDate=", "-ModifyDate=2024:05:01 14:30:00"]],
  ["sample-future-2099.jpg", ["-DateTimeOriginal=2099:01:01 00:00:00", "-CreateDate=", "-ModifyDate="]],
  ["sample-pre-1995.jpg", ["-DateTimeOriginal=1980:01:01 00:00:00", "-CreateDate=", "-ModifyDate="]],
];

const FILENAME_VARIANTS = [
  "IMG_20240501_143000.jpg",
  "DSC_20240501_143000.jpg",
  "Screenshot_2024-05-17-12-00-00.jpg",
  "1715961600000.jpg",
];

const BUN_GENERATORS = [
  "gen_png_exif.ts",
  "gen_jpeg_makernotes_broken.ts",
  "gen_rw2.ts",
  "gen_ooxml.ts",
  "gen_pdf.ts",
  "gen_odf.ts",
  "gen_rtf.ts",
  "gen_epub.ts",
];

const XMP = `<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/"
                     photoshop:DateCreated="2024-05-01T14:30:00+08:00"/>
  </rdf:RDF>
</x:xmpmeta>
`;

const TAKEOUT = `{"photoTakenTime":{"timestamp":"1714576200","formatted":"May 1, 2024, 2:30:00 PM UTC"}}\n`;

function run(cmd: string[]): void {
  const done = Bun.spawnSync(cmd, { cwd: DATA_DIR, stdio: ["inherit", "inherit", "inherit"] });
  if (done.exitCode !== 0) {
    console.error(`failed: ${cmd.join(" ")}`);
    process.exit(done.exitCode ?? 1);
  }
}

function data(name: string): string {
  return resolve(DATA_DIR, name);
}

function genExifVariants(): void {
  for (const [name, tags] of EXIF_VARIANTS) {
    copyFileSync(data(BASE), data(name));
    run(["exiftool", "-q", "-overwrite_original", ...tags, name]);
  }
}

function genFilenameVariants(): void {
  copyFileSync(data("sample-no-dates.jpg"), data(BLANK));
  try {
    for (const name of FILENAME_VARIANTS) {
      copyFileSync(data(BLANK), data(name));
      appendFileSync(data(name), `\0${name}`);
    }
  } finally {
    rmSync(data(BLANK), { force: true });
  }
}

function main(): void {
  genExifVariants();
  run([
    "ffmpeg", "-y", "-loglevel", "error", "-f", "lavfi", "-i", "color=s=8x8:d=0.1", "-t", "0.1",
    "-pix_fmt", "yuv420p", "sample-mp4-1904-epoch.mp4",
  ]);
  genFilenameVariants();
  writeFileSync(data("sample-xmp.xmp"), XMP);
  writeFileSync(data("photo-takeout.json"), TAKEOUT);
  for (const script of BUN_GENERATORS) run(["bun", resolve(SCRIPT_DIR, script)]);
  console.log(
    "NOTE: regenerate doc/xls/ppt via: cargo test --release --test gen_cfb_fixtures && cp target/cfb-fixtures/sample-* tests/data/",
  );
  console.log(`Generated ${readdirSync(DATA_DIR).length} files in ${DATA_DIR}`);
}

main();
