// 生成带 `eXIf` chunk 的最小 PNG fixture。
//
// 产物：tests/data/sample-png-exif.png（一次性，commit 到 git；运行期不依赖 Bun）。
//
// eXIf chunk 是 PNG 1.5+ 标准，内嵌完整 TIFF/EXIF header（与 JPEG APP1 段
// 后半段同结构）。nom-exif 3.6 不解析此 chunk，归档走自实现路径。
//
// EXIF 内容：IFD0 Make=Canon, Model=EOS 7D, ExifIFDPointer；ExifIFD
// DateTimeOriginal/CreateDate/ModifyDate（2017-02-14 10:30:00..02）。
// 时间选 2017-02 让 DTO 与 fixture mtime（FIXED_MEDIA_MTIME=2024-01-01）必然不同。

import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { crc32 } from "./lib/zip.ts";

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

function u32be(v: number): Buffer {
  const b = Buffer.alloc(4);
  b.writeUInt32BE(v >>> 0, 0);
  return b;
}

function ifdEntry(tag: number, typ: number, cnt: number, val: number): Buffer {
  return Buffer.concat([u16le(tag), u16le(typ), u32le(cnt), u32le(val)]);
}

// 与 gen_rw2.ts 的 buildRw2Payload 同构（共享 IFD 布局），仅 magic 与数据区起点差异。
function buildExifPayload(): Buffer {
  const DTO_OFF = 92;
  const CREATE_OFF = 112;
  const MODIFY_OFF = 132;
  const MAKE_OFF = 152;
  const MODEL_OFF = 158;
  return Buffer.concat([
    Buffer.from("II"),
    u16le(0x002a),
    u32le(8),
    u16le(3),
    ifdEntry(0x010f, 2, MAKE_STR.length, MAKE_OFF),
    ifdEntry(0x0110, 2, MODEL_STR.length, MODEL_OFF),
    ifdEntry(0x8769, 4, 1, 50),
    u32le(0),
    u16le(3),
    ifdEntry(0x9003, 2, DTO_STR.length, DTO_OFF),
    ifdEntry(0x9004, 2, CREATE_DATE_STR.length, CREATE_OFF),
    ifdEntry(0x0132, 2, MODIFY_DATE_STR.length, MODIFY_OFF),
    u32le(0),
    DTO_STR,
    CREATE_DATE_STR,
    MODIFY_DATE_STR,
    MAKE_STR,
    MODEL_STR,
  ]);
}

function pngChunk(chunkType: Buffer, data: Buffer): Buffer {
  const crc = crc32(Buffer.concat([chunkType, data]));
  return Buffer.concat([u32be(data.length), chunkType, data, u32be(crc)]);
}

function buildMinimalPng(): Buffer {
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.concat([
    u32be(1),
    u32be(1),
    Buffer.from([8, 0, 0, 0, 0]), // 1×1 grayscale 8-bit
  ]);
  const idat = deflateSync(Buffer.from([0x00, 0x00]));
  return Buffer.concat([
    sig,
    pngChunk(Buffer.from("IHDR"), ihdr),
    pngChunk(Buffer.from("eXIf"), buildExifPayload()),
    pngChunk(Buffer.from("IDAT"), idat),
    pngChunk(Buffer.from("IEND"), Buffer.alloc(0)),
  ]);
}

const out = join(dirname(fileURLToPath(import.meta.url)), "..", "data", "sample-png-exif.png");
mkdirSync(dirname(out), { recursive: true });
const payload = buildMinimalPng();
writeFileSync(out, payload);
console.log(`wrote ${out} (${payload.length} bytes)`);
