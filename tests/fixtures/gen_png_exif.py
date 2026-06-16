"""生成带 `eXIf` chunk 的最小 PNG fixture。

产物：tests/data/sample-png-exif.png（一次性，commit 到 git；运行期不依赖 Python）。

eXIf chunk 是 PNG 1.5+ 标准（W3C PNG 1.5），内嵌完整 TIFF/EXIF header（与 JPEG
APP1 段后半段同结构）。nom-exif 3.6 不解析此 chunk，归档需走自实现路径。

EXIF 内容：
- IFD0: Make="Canon", Model="EOS 7D", ExifIFDPointer → ExifIFD
- ExifIFD: DateTimeOriginal=2017:02:14 10:30:00, CreateDate=2017:02:14 10:30:01,
           ModifyDate=2017:02:14 10:30:02

时间选 2017-02 让 DTO 与 fixture mtime（FIXED_MEDIA_MTIME=2024-01-01）必然不同，
便于断言"走 EXIF 不走 mtime"。
"""

from __future__ import annotations

import struct
import sys
import zlib
from pathlib import Path

# Fixture 时间字面量：与 FIXED_MEDIA_MTIME=2024-01-01 不同，验证 P0 命中而非 mtime 兜底。
DTO_STR = b"2017:02:14 10:30:00\0"
CREATE_DATE_STR = b"2017:02:14 10:30:01\0"
MODIFY_DATE_STR = b"2017:02:14 10:30:02\0"
MAKE_STR = b"Canon\0"
MODEL_STR = b"EOS 7D\0"


def _u16le(v: int) -> bytes:
    return struct.pack("<H", v)


def _u32le(v: int) -> bytes:
    return struct.pack("<I", v)


def build_exif_payload() -> bytes:
    """构造 eXIf chunk payload = 完整 TIFF/EXIF header（little-endian）。

    布局（offset 相对 payload 起点）：
      0..8   : TIFF header ("II" + magic 0x002A + IFD0 offset=8)
      8..10  : IFD0 count = 3
      10..46 : 3 entries × 12  (Make, Model, ExifIFDPointer)
      46..50 : IFD0 next-offset = 0
      50..52 : ExifIFD count = 3
      52..88 : 3 entries × 12  (DTO, CreateDate, ModifyDate)
      88..92 : ExifIFD next-offset = 0
      92..112 : DTO ASCII data
      112..132: CreateDate ASCII data
      132..152: ModifyDate ASCII data
      152..158: Make ASCII data
      158..165: Model ASCII data
    """
    # IFD entry helper: tag(u16) + typ(u16) + cnt(u32) + val(u32) = 12 字节
    def entry(tag: int, typ: int, cnt: int, val: int) -> bytes:
        return _u16le(tag) + _u16le(typ) + _u32le(cnt) + _u32le(val)

    # 数据 offset 常量
    DTO_OFF = 92
    CREATE_OFF = 112
    MODIFY_OFF = 132
    MAKE_OFF = 152
    MODEL_OFF = 158

    parts: list[bytes] = []
    # TIFF header
    parts.append(b"II")
    parts.append(_u16le(0x002A))
    parts.append(_u32le(8))
    # IFD0
    parts.append(_u16le(3))
    parts.append(entry(0x010F, 2, len(MAKE_STR), MAKE_OFF))   # Make
    parts.append(entry(0x0110, 2, len(MODEL_STR), MODEL_OFF))  # Model
    parts.append(entry(0x8769, 4, 1, 50))                      # ExifIFDPointer → 50
    parts.append(_u32le(0))                                    # next IFD = 0
    # ExifIFD
    parts.append(_u16le(3))
    parts.append(entry(0x9003, 2, len(DTO_STR), DTO_OFF))            # DateTimeOriginal
    parts.append(entry(0x9004, 2, len(CREATE_DATE_STR), CREATE_OFF))  # CreateDate
    parts.append(entry(0x0132, 2, len(MODIFY_DATE_STR), MODIFY_OFF))  # ModifyDate
    parts.append(_u32le(0))
    # ASCII data
    parts.append(DTO_STR)
    parts.append(CREATE_DATE_STR)
    parts.append(MODIFY_DATE_STR)
    parts.append(MAKE_STR)
    parts.append(MODEL_STR)
    return b"".join(parts)


def png_chunk(chunk_type: bytes, data: bytes) -> bytes:
    """PNG chunk = length(u32 BE) + type(4B) + data + CRC32(u32 BE，覆盖 type+data)。"""
    crc = zlib.crc32(chunk_type + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + chunk_type + data + struct.pack(">I", crc)


def build_minimal_png() -> bytes:
    sig = b"\x89PNG\r\n\x1a\n"
    # IHDR 13 bytes: width(4) height(4) bit_depth(1) color_type(1) compression(1) filter(1) interlace(1)
    ihdr = struct.pack(">IIBBBBB", 1, 1, 8, 0, 0, 0, 0)  # 1×1 grayscale 8-bit
    # 最小合法 IDAT：zlib 压缩的 2-byte 灰度像素（filter byte 0 + 1 byte data）
    idat = zlib.compress(b"\x00\x00")
    payload = build_exif_payload()
    return b"".join([
        sig,
        png_chunk(b"IHDR", ihdr),
        png_chunk(b"eXIf", payload),
        png_chunk(b"IDAT", idat),
        png_chunk(b"IEND", b""),
    ])


def main() -> int:
    out = Path(__file__).resolve().parents[1] / "data" / "sample-png-exif.png"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(build_minimal_png())
    print(f"wrote {out} ({out.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
