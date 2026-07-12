#!/usr/bin/env python3
"""模板改名：把仓库内全部模板 crate 名替换为新项目名（clone 后必改清单第 1 项）。

用法:
    uv run tools/rename.py <new_crate_name>   # 或 python3 tools/rename.py <new_crate_name>

替换后自动跑 cargo check 验证；CLAUDE.md 中的叙述性文字请人工复核。
"""

import re
import subprocess
import sys
from pathlib import Path

# 模板名拼接构造，避免本脚本自身被下一次改名误替换
TEMPLATE_NAME = "skel" + "_rs"
SKIP_DIRS = {".git", "target", "node_modules"}
SKIP_FILES = {"Cargo.lock"}


def iter_text_files(root: Path):
    for p in root.rglob("*"):
        if not p.is_file() or p.name in SKIP_FILES:
            continue
        if any(part in SKIP_DIRS for part in p.parts):
            continue
        if p.resolve() == Path(__file__).resolve():
            continue
        yield p


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    new_name = sys.argv[1]
    if not re.fullmatch(r"[a-z][a-z0-9_]*", new_name):
        print(f"cannot rename: {new_name!r} 不是合法 crate 名（^[a-z][a-z0-9_]*$）", file=sys.stderr)
        return 2
    if new_name == TEMPLATE_NAME:
        print("cannot rename: 新名与模板名相同", file=sys.stderr)
        return 2

    root = Path(__file__).resolve().parent.parent
    changed = []
    for p in iter_text_files(root):
        try:
            text = p.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        if TEMPLATE_NAME not in text:
            continue
        count = text.count(TEMPLATE_NAME)
        p.write_text(text.replace(TEMPLATE_NAME, new_name), encoding="utf-8")
        changed.append((p.relative_to(root), count))

    if not changed:
        print(f"未发现 {TEMPLATE_NAME}，无需替换（可能已改名）")
        return 0

    for rel, count in sorted(changed):
        print(f"  {rel}: {count} 处")
    print(f"共 {len(changed)} 个文件替换为 {new_name!r}，运行 cargo check 验证…")

    check = subprocess.run(
        ["cargo", "check", "--release", "--workspace", "--features", "http,sqlite"],
        cwd=root,
    )
    if check.returncode != 0:
        print("cargo check 失败，请检查上方输出", file=sys.stderr)
        return 1

    print("完成。后续步骤见 CLAUDE.md「clone 后必改清单」2-7 项（业务实体/migrations/配置等）。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
