// 生成最小 Panasonic RW2 RAW fixture（TIFF 变体，magic `0x0055`）。
//
// 产物：tests/data/sample-rw2.rw2（一次性，commit 到 git；运行期不依赖 Bun）。
//
// RW2 header 与 TIFF 同布局（BOM + magic + IFD0 offset）但 magic 为 `0x0055`
// （`II U\0` = 0x49 0x49 0x55 0x00）而非 TIFF `0x002A`；infer 0.19 不识 → sniff
// 为空 → `mime_from_ext("rw2")` 兜底为 `image/x-panasonic-rw2` → `entities::rw2`
// 自解析。内容与 gen_png_exif.py 的 eXIf payload 同构（IFD0 + ExifIFD 链），
// 只是不包 PNG chunk，文件内容直接是 TIFF header + IFD。
//
// EXIF 内容：
// - IFD0: Make="Canon", Model="EOS 7D", ExifIFDPointer → ExifIFD
// - ExifIFD: DateTimeOriginal=2017:02:14 10:30:00, CreateDate=2017:02:14 10:30:01,
//            ModifyDate=2017:02:14 10:30:02
//
// 时间选 2017-02 让 DTO 与 fixture mtime（FIXED_MEDIA_MTIME=2024-01-01）必然不同，
// 便于断言"走 EXIF 不走 mtime"。

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const DTO_STR = Buffer.from("2017:02:14 10:30:00\0");
const CREATE_DATE_STR = Buffer.from("2017:02:14 10:30:01\0");
const MODIFY_DATE_STR = Buffer.from("2017:02:14 10:30:02\0");
const MAKE_STR = Buffer.from("Canon\0");
const MODEL_STR = Buffer.from("EOS 7D\0");

function u16le(v: number): Buffer {
  const b = Buffer.alloc(2);
  b.writeUInt16LE(v, 0);
  return b;
}

function u32le(v: number): Buffer {
  const b = Buffer.alloc(4);
  b.writeUInt32LE(v >>> 0, 0);
  return b;
}

function entry(tag: number, typ: number, cnt: number, val: number): Buffer {
  return Buffer.concat([u16le(tag), u16le(typ), u32le(cnt), u32le(val)]);
}

// 布局（offset 相对文件起点）：
//   0..8   : TIFF header ("II" + magic 0x0055 + IFD0 offset=8)
//   8..10  : IFD0 count = 3
//   10..46 : 3 entries × 12  (Make, Model, ExifIFDPointer)
//   46..50 : IFD0 next-offset = 0
//   50..52 : ExifIFD count = 3
//   52..88 : 3 entries × 12  (DTO, CreateDate, ModifyDate)
//   88..92 : ExifIFD next-offset = 0
//   92..112 : DTO ASCII data
//   112..132: CreateDate ASCII data
//   132..152: ModifyDate ASCII data
//   152..158: Make ASCII data
//   158..165: Model ASCII data
function buildRw2Payload(): Buffer {
  const DTO_OFF = 92;
  const CREATE_OFF = 112;
  const MODIFY_OFF = 132;
  const MAKE_OFF = 152;
  const MODEL_OFF = 158;

  return Buffer.concat([
    Buffer.from("II"),
    u16le(0x0055),
    u32le(8),
    u16le(3),
    entry(0x010f, 2, MAKE_STR.length, MAKE_OFF),
    entry(0x0110, 2, MODEL_STR.length, MODEL_OFF),
    entry(0x8769, 4, 1, 50),
    u32le(0),
    u16le(3),
    entry(0x9003, 2, DTO_STR.length, DTO_OFF),
    entry(0x9004, 2, CREATE_DATE_STR.length, CREATE_OFF),
    entry(0x0132, 2, MODIFY_DATE_STR.length, MODIFY_OFF),
    u32le(0),
    DTO_STR,
    CREATE_DATE_STR,
    MODIFY_DATE_STR,
    MAKE_STR,
    MODEL_STR,
  ]);
}

const out = join(dirname(fileURLToPath(import.meta.url)), "..", "data", "sample-rw2.rw2");
mkdirSync(dirname(out), { recursive: true });
const payload = buildRw2Payload();
writeFileSync(out, payload);
console.log(`wrote ${out} (${payload.length} bytes)`);
