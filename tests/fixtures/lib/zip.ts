// 最小 ZIP 容器写入器（供 gen_ooxml/gen_odf/gen_epub 合成 OPC/ODF/EPUB fixture）。
//
// Bun 无内置 zip writer；按 ZIP 规范手写 Local File Header + Central Directory
// + EOCD。deflate 用 node:zlib.deflateRawSync（ZIP 存储的是 raw deflate 流）。
// CRC-32 对原始数据计算，Rust zip crate 解析时校验，必须正确。

import { deflateRawSync } from "node:zlib";

export interface ZipEntry {
  name: string;
  data: Uint8Array;
  /** true = stored 不压缩（ODF/EPUB 的 mimetype 必须是 stored 首 entry）。 */
  stored?: boolean;
}

/** CRC-32（IEEE 802.3，多项式 0xEDB88320），返回无符号 32 位。 */
export function crc32(buf: Uint8Array): number {
  let crc = 0xffffffff;
  for (const byte of buf) {
    crc ^= byte;
    for (let i = 0; i < 8; i++) {
      crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

/** DOS 日期时间（ZIP header 用）。固定 1980-01-01 00:00:00，zip 解析不关心。 */
const DOS_DATE = 0x21;
const DOS_TIME = 0;

/** 把一个 ZIP entry 拼成 Local File Header + data。返回 (chunk, size)。 */
function localChunk(e: ZipEntry, nameBuf: Uint8Array): [Uint8Array, number] {
  const data = e.stored ? e.data : deflateRawSync(e.data);
  const method = e.stored ? 0 : 8;
  const head = new Uint8Array(30);
  const dv = new DataView(head.buffer);
  dv.setUint32(0, 0x0403_4b50, true);
  dv.setUint16(4, 20, true); // version needed
  dv.setUint16(6, 0x0800, true); // UTF-8 flag
  dv.setUint16(8, method, true);
  dv.setUint16(10, DOS_TIME, true);
  dv.setUint16(12, DOS_DATE, true);
  dv.setUint32(14, crc32(e.data), true);
  dv.setUint32(18, data.length, true);
  dv.setUint32(22, e.data.length, true);
  dv.setUint16(26, nameBuf.length, true);
  dv.setUint16(28, 0, true); // extra len
  const chunk = new Uint8Array(30 + nameBuf.length + data.length);
  chunk.set(head, 0);
  chunk.set(nameBuf, 30);
  chunk.set(data, 30 + nameBuf.length);
  return [chunk, chunk.length];
}

/** Central Directory header for an entry。 */
function centralChunk(e: ZipEntry, nameBuf: Uint8Array, localOffset: number): Uint8Array {
  const data = e.stored ? e.data : deflateRawSync(e.data);
  const method = e.stored ? 0 : 8;
  const head = new Uint8Array(46);
  const dv = new DataView(head.buffer);
  dv.setUint32(0, 0x0201_4b50, true);
  dv.setUint16(4, 20, true); // version made by
  dv.setUint16(6, 20, true); // version needed
  dv.setUint16(8, 0x0800, true);
  dv.setUint16(10, method, true);
  dv.setUint16(12, DOS_TIME, true);
  dv.setUint16(14, DOS_DATE, true);
  dv.setUint32(16, crc32(e.data), true);
  dv.setUint32(20, data.length, true);
  dv.setUint32(24, e.data.length, true);
  dv.setUint16(28, nameBuf.length, true);
  dv.setUint16(30, 0, true); // extra len
  dv.setUint16(32, 0, true); // comment len
  dv.setUint16(34, 0, true); // disk start
  dv.setUint16(36, 0, true); // internal attrs
  dv.setUint32(38, 0, true); // external attrs
  dv.setUint32(42, localOffset, true); // local header offset
  const chunk = new Uint8Array(46 + nameBuf.length);
  chunk.set(head, 0);
  chunk.set(nameBuf, 46);
  return chunk;
}

/** 组装完整 ZIP 字节流。entry 顺序保持调用方传入（ODF/EPUB mimetype 须首位）。 */
export function buildZip(entries: ZipEntry[]): Uint8Array {
  const parts: Uint8Array[] = [];
  const central: Uint8Array[] = [];
  let offset = 0;
  for (const e of entries) {
    const nameBuf = new TextEncoder().encode(e.name);
    const [chunk, len] = localChunk(e, nameBuf);
    parts.push(chunk);
    central.push(centralChunk(e, nameBuf, offset));
    offset += len;
  }
  const cd = concatBytes(central);
  const cdStart = offset;
  const eocd = new Uint8Array(22);
  const dv = new DataView(eocd.buffer);
  dv.setUint32(0, 0x0605_4b50, true);
  dv.setUint16(4, 0, true);
  dv.setUint16(6, 0, true);
  dv.setUint16(8, entries.length, true);
  dv.setUint16(10, entries.length, true);
  dv.setUint32(12, cd.length, true);
  dv.setUint32(16, cdStart, true);
  dv.setUint16(20, 0, true); // comment len
  return concatBytes([...parts, cd, eocd]);
}

function concatBytes(chunks: Uint8Array[]): Uint8Array {
  const total = chunks.reduce((n, c) => n + c.length, 0);
  const out = new Uint8Array(total);
  let off = 0;
  for (const c of chunks) {
    out.set(c, off);
    off += c.length;
  }
  return out;
}
