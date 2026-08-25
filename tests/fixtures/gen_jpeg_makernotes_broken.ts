// 生成合成 JPEG fixture：nom-exif `parse_exif` 失败但 IFD0 仍可读。
//
// 产物：tests/data/sample-jpeg-app1-broken.jpg（一次性，commit 到 git）。
//
// 模拟 Canon EOS 7D MakerNotes 偏移异常场景：nom-exif 整体 `parse_exif` 返 Err。
// APP1 Exif IFD0 含 Make/Model/ExifIFDPointer；ExifIFD 声称 count=10000 但实际
// 只有 1 entry 的空间，使 nom-exif 越界拒绝；自实现 fallback 保留 IFD0 字段。

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

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

// TIFF: IFD0 含 Make/Model/ExifIFDPointer(→损坏 ExifIFD)。
function buildBrokenTiff(): Buffer {
  const parts: Buffer[] = [];
  parts.push(Buffer.from("II"), u16le(0x002a), u32le(8));
  parts.push(u16le(3)); // IFD0 count
  parts.push(Buffer.concat([u16le(0x010f), u16le(2), u32le(6), u32le(80)])); // Make
  parts.push(Buffer.concat([u16le(0x0110), u16le(2), u32le(7), u32le(86)])); // Model
  parts.push(Buffer.concat([u16le(0x8769), u16le(4), u32le(1), u32le(50)])); // ExifIFDPointer
  parts.push(u32le(0)); // IFD0 next；至此 cum = 8+2+36+4 = 50
  parts.push(u16le(10000)); // 恶意 ExifIFD @50：count=10000
  parts.push(Buffer.concat([u16le(0x9003), u16le(2), u32le(20), u32le(93)])); // DTO entry
  parts.push(Buffer.alloc(80 - 64)); // 填充，让后续 entries 越界
  parts.push(Buffer.from("Cam\0\0\0")); // 80..86
  parts.push(Buffer.from("Model\0\0")); // 86..93
  parts.push(Buffer.from("2017:02:14 10:30:00\0")); // 93..113
  return Buffer.concat(parts);
}

function jpegWithApp1(tiff: Buffer): Buffer {
  const payload = Buffer.concat([Buffer.from("Exif\0\0"), tiff]);
  const segLen = payload.length + 2; // length 字段含自身 2 字节
  const seg = Buffer.alloc(2);
  seg.writeUInt16BE(segLen, 0);
  return Buffer.concat([Buffer.from([0xff, 0xd8, 0xff, 0xe1]), seg, payload, Buffer.from([0xff, 0xd9])]);
}

const out = join(dirname(fileURLToPath(import.meta.url)), "..", "data", "sample-jpeg-app1-broken.jpg");
mkdirSync(dirname(out), { recursive: true });
const payload = jpegWithApp1(buildBrokenTiff());
writeFileSync(out, payload);
console.log(`wrote ${out} (${payload.length} bytes)`);
