#!/usr/bin/env python
"""Step 4.5：copied 文件与目标库三层内容比对（SHA-512 / 像素流 hash / 同名候选收集）。

Usage: uv run --quiet --no-project 45_check_copied.py [work_dir=/tmp/tm] [dst_root]
输入: <work>/copy_lines.log（01_dry_run.sh 产物）
输出: <work>/check_copied.tsv（verdict\tsrc\ttgt\thit）+ stdout 汇总
verdict: EXACT_DUP / PIXEL_SAME / NAME_ONLY（需再跑旋转校正 pHash）/ ABSENT
"""
import sys, os, re, struct, hashlib, collections

sys.stdout.reconfigure(newline="\n")
WORK = sys.argv[1] if len(sys.argv) > 1 else r"C:\Users\f\AppData\Local\Temp\tm"
DROOT = sys.argv[2] if len(sys.argv) > 2 else r"D:\Users\Public\Pictures"
CHUNK = 1 << 20


def sha512(p):
    h = hashlib.sha512()
    with open(p, "rb") as fh:
        for blk in iter(lambda: fh.read(CHUNK), b""):
            h.update(blk)
    return h.hexdigest()


def jpeg_scan_hash(path):
    h = hashlib.sha512()
    with open(path, "rb") as fh:
        if fh.read(2) != b"\xff\xd8":
            return None
        while True:
            b = fh.read(1)
            if not b:
                return None
            if b != b"\xff":
                continue
            marker = fh.read(1)
            while marker == b"\xff":
                marker = fh.read(1)
            if marker in (b"\xd8", b"\x01") or b"\xd0" <= marker <= b"\xd7":
                continue
            seg_len = struct.unpack(">H", fh.read(2))[0]
            if marker == b"\xda":
                fh.read(seg_len - 2)
                for blk in iter(lambda: fh.read(CHUNK), b""):
                    h.update(blk)
                return h.hexdigest()
            fh.seek(seg_len - 2, os.SEEK_CUR)


def png_idat_hash(path):
    h = hashlib.sha512()
    found = False
    with open(path, "rb") as fh:
        if fh.read(8) != b"\x89PNG\r\n\x1a\n":
            return None
        while True:
            hdr = fh.read(8)
            if len(hdr) < 8:
                break
            ln, typ = struct.unpack(">I", hdr[:4])[0], hdr[4:]
            if typ == b"IDAT":
                found = True
                h.update(fh.read(ln))
                fh.read(4)
            else:
                fh.seek(ln + 4, os.SEEK_CUR)
            if typ == b"IEND":
                break
    return h.hexdigest() if found else None


def box_mdat_hash(path):
    h = hashlib.sha512()
    found = False
    size = os.path.getsize(path)
    with open(path, "rb") as fh:
        pos = 0
        while pos + 8 <= size:
            fh.seek(pos)
            hdr = fh.read(8)
            if len(hdr) < 8:
                break
            ln32, typ = struct.unpack(">I", hdr[:4])[0], hdr[4:]
            if ln32 == 1:
                ln = struct.unpack(">Q", fh.read(8))[0]
                hdr_len = 16
            elif ln32 == 0:
                ln = size - pos
                hdr_len = 8
            else:
                ln = ln32
                hdr_len = 8
            if ln < 8:
                break
            if typ == b"mdat":
                found = True
                remaining = ln - hdr_len
                while remaining > 0:
                    blk = fh.read(min(CHUNK, remaining))
                    if not blk:
                        break
                    h.update(blk)
                    remaining -= len(blk)
            pos += ln
    return h.hexdigest() if found else None


def content_hash(path):
    ext = os.path.splitext(path)[1].lower()
    try:
        if ext in (".jpg", ".jpeg"):
            return jpeg_scan_hash(path) or png_idat_hash(path)
        if ext == ".png":
            return png_idat_hash(path) or jpeg_scan_hash(path)
        if ext in (".mp4", ".mov", ".m4v", ".3gp", ".heic", ".heif"):
            return box_mdat_hash(path)
    except OSError:
        return None
    return None


def main():
    srcs = []
    for line in open(os.path.join(WORK, "copy_lines.log"), encoding="utf-8"):
        m = re.search(r"source=(\S.*?) target=(\S.*?)$", line.rstrip("\n"))
        if m:
            srcs.append((m.group(1), m.group(2)))
    print(f"copied={len(srcs)}")

    by_name = collections.defaultdict(list)
    for dirpath, _dirs, files in os.walk(DROOT):
        for f in files:
            by_name[f.lower()].append(os.path.join(dirpath, f))

    suffix_re = re.compile(r"^(.*)_\d+$")
    rows = []
    for src, tgt in srcs:
        base = os.path.basename(src)
        stem, ext = os.path.splitext(base)
        cands = list(by_name.get(base.lower(), []))
        for key, paths in by_name.items():
            m2 = suffix_re.match(key)
            if m2 and m2.group(1) == stem.lower():
                cands.extend(p for p in paths if p.lower().endswith(ext.lower()))
        sh_sha, sh_pix = sha512(src), content_hash(src)
        verdict, hit = "ABSENT", ""
        for d in cands:
            if sha512(d) == sh_sha:
                verdict, hit = "EXACT_DUP", d
                break
            if sh_pix and content_hash(d) == sh_pix:
                verdict, hit = "PIXEL_SAME", d
                break
        if verdict == "ABSENT" and cands:
            verdict, hit = "NAME_ONLY", "|".join(cands)
        rows.append((verdict, src, tgt, hit))
        print(f"{verdict:11s}  {src}" + (f"  hit={hit}" if hit else ""))

    with open(os.path.join(WORK, "check_copied.tsv"), "w", encoding="utf-8", newline="\n") as fh:
        for v, s, t, h in rows:
            fh.write(f"{v}\t{s}\t{t}\t{h}\n")
    stat = collections.Counter(r[0] for r in rows)
    print("---summary---")
    for k, n in stat.most_common():
        print(f"{n:5d}  {k}")


main()
