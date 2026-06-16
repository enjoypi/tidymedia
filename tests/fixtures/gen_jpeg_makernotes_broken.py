"""生成合成 JPEG fixture：nom-exif `parse_exif` 失败但 IFD0 仍可读。

产物：tests/data/sample-jpeg-app1-broken.jpg（一次性，commit 到 git）。

模拟 Canon EOS 7D MakerNotes 偏移异常场景（exiftool 报 `Adjusted MakerNotes
base by -126`）：nom-exif 整体 `parse_exif` 返 Err，丢失全部时间字段。

合成策略：APP1 Exif IFD0 含 Make/Model/ExifIFDPointer；ExifIFD 声称 count=10000
但实际只有 1 entry 的空间，使 nom-exif 在扫 ExifIFD 时越界拒绝；自实现 fallback
通过 `let count = u16_at(...)?` + 后续 `?` 同样越界，但 IFD0 字段已读出。

注意：tidymedia fallback 实际只读 IFD0 + 一个 ExifIFD；ExifIFD 越界时
`scan_ifd` 内 `?` 触发 None → `parse_ifds` 把它当 ExifIFD 子调用失败忽略，
保留 IFD0 字段。所以 IFD0 的 Make/Model + ExifIFD 头一个 entry 的 DTO 都能读。
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path


def _u16le(v: int) -> bytes:
    return struct.pack("<H", v)


def _u32le(v: int) -> bytes:
    return struct.pack("<I", v)


def build_broken_tiff() -> bytes:
    """TIFF: IFD0 含 Make/Model/ExifIFDPointer(→损坏 ExifIFD)。"""
    out = b"II" + _u16le(0x002A) + _u32le(8)
    # IFD0 count=3
    out += _u16le(3)
    # Make @offset 80 (cnt=6: "Cam\0\0\0")
    out += _u16le(0x010F) + _u16le(2) + _u32le(6) + _u32le(80)
    # Model @offset 86 (cnt=7: "Model\0\0")
    out += _u16le(0x0110) + _u16le(2) + _u32le(7) + _u32le(86)
    # ExifIFDPointer @offset 50 (恶意 ExifIFD)
    out += _u16le(0x8769) + _u16le(4) + _u32le(1) + _u32le(50)
    out += _u32le(0)  # IFD0 next-IFD = 0；至此 cum = 8+2+36+4 = 50
    # 恶意 ExifIFD @50：声称 count=10000，但仅放 1 个 DTO entry 的空间
    out += _u16le(10000)  # 50..52
    # DTO entry 52..64：val=93（数据区）
    out += _u16le(0x9003) + _u16le(2) + _u32le(20) + _u32le(93)
    # 填充 64..80 让后续 9998 entries 越界（位于文件末尾外）
    out += b"\x00" * (80 - 64)
    # ASCII 数据
    out += b"Cam\0\0\0"  # 80..86
    out += b"Model\0\0"  # 86..93
    out += b"2017:02:14 10:30:00\0"  # 93..113
    return out


def jpeg_with_app1(tiff: bytes) -> bytes:
    payload = b"Exif\0\0" + tiff
    seg_len = len(payload) + 2  # length 字段含自身 2 字节
    return b"\xff\xd8\xff\xe1" + struct.pack(">H", seg_len) + payload + b"\xff\xd9"


def main() -> int:
    out = Path(__file__).resolve().parents[1] / "data" / "sample-jpeg-app1-broken.jpg"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(jpeg_with_app1(build_broken_tiff()))
    print(f"wrote {out} ({out.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
