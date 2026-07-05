#!/usr/bin/env python3
"""tidymedia 真实数据性能采集：一次跑产 AI 可读性能报告。

用法：
    uv run --quiet --no-project scripts/perf-collect.py \\
        --sub copy --data /path/to/photos --output-dir /tmp/perf-run

产物（--output-dir 下）：
    report.json     tidymedia --report 落地的 Report JSON（含 duration_ms）
    time-v.txt      /usr/bin/time -v 抓的 RSS/CPU/IO 统计原文
    perf-report.md  单一汇总 markdown，直接扔给 LLM 分析

仅用 Python 标准库：argparse / subprocess / json / re / pathlib / sys / os / shutil。
"""

# ruff: noqa: T201
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

if sys.stdout.encoding is None or "utf" not in sys.stdout.encoding.lower():
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
else:
    sys.stdout.reconfigure(newline="\n")


SUBS = ["copy", "move", "find", "cull", "move-text-shot"]

TIME_V_FIELDS = [
    ("Maximum resident set size (kbytes):", "max_rss_kb", int),
    ("Elapsed (wall clock) time (h:mm:ss or m:ss):", "elapsed_wall", str),
    ("User time (seconds):", "user_time_sec", float),
    ("System time (seconds):", "system_time_sec", float),
    ("Percent of CPU this job got:", "cpu_percent", str),
    ("File system inputs:", "fs_inputs", int),
    ("File system outputs:", "fs_outputs", int),
    ("Voluntary context switches:", "vol_ctx_switches", int),
    ("Involuntary context switches:", "invol_ctx_switches", int),
    ("Page size (bytes):", "page_size_bytes", int),
    ("Minor (reclaiming a frame) page faults:", "minor_page_faults", int),
    ("Major (requiring I/O) page faults:", "major_page_faults", int),
]


def parse_time_v(text: str) -> dict:
    """从 /usr/bin/time -v stderr 抽结构化字段。缺失字段静默 skip。"""
    result: dict = {}
    for line in text.splitlines():
        stripped = line.strip()
        for prefix, name, cast in TIME_V_FIELDS:
            if stripped.startswith(prefix):
                raw = stripped[len(prefix):].strip()
                try:
                    result[name] = cast(raw) if cast is not str else raw
                except ValueError:
                    result[name] = raw
                break
    return result


def parse_iso_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def find_binary(project_root: Path) -> Path:
    """tidymedia release binary 位置。仅本地开发，不搜 $PATH。"""
    candidate = project_root / "target" / "release" / "tidymedia"
    if candidate.exists():
        return candidate
    print(f"error: {candidate} not found; build first with cargo build --release",
          file=sys.stderr)
    sys.exit(1)


def find_gnu_time() -> str | None:
    """Linux 走 /usr/bin/time；macOS 走 gtime（brew install gnu-time）。"""
    for path in ("/usr/bin/time", "/usr/local/bin/gtime", "/opt/homebrew/bin/gtime"):
        if Path(path).exists():
            return path
    if shutil.which("gtime"):
        return "gtime"
    return None


def build_cli(sub: str, data: str, extra: list[str], report_path: Path,
              output_target: str | None) -> list[str]:
    """按 sub 组装 tidymedia CLI 参数。output 对多数子命令必填，find 除外。"""
    args = [sub]
    if sub == "find":
        args += [data]
        if output_target:
            args += ["-o", output_target]
    elif sub == "cull":
        args += [data]
        if output_target:
            args += ["-o", output_target]
    elif sub == "move-text-shot":
        args += ["--dry-run", data]
        if output_target:
            args += ["-o", output_target]
    else:  # copy / move
        args += ["--dry-run", data]
        if output_target:
            args += ["-o", output_target]
    args += ["--report", str(report_path)]
    args += extra
    return args


def run_with_time_v(time_bin: str, tidy_bin: Path, cli_args: list[str],
                    time_v_out: Path, env: dict) -> int:
    """通过 /usr/bin/time -v 包裹 tidymedia；stderr 走 time -v 输出。"""
    cmd = [time_bin, "-v", str(tidy_bin), *cli_args]
    with time_v_out.open("wb") as ferr:
        proc = subprocess.run(cmd, stderr=ferr, env=env, check=False)  # noqa: S603
    return proc.returncode


def render_report(sub: str, data: str, report_json: dict, time_v: dict,
                  return_code: int, output_dir: Path) -> str:
    """生成 markdown 汇总；字段缺失时输出 `n/a`，不 raise。"""
    duration_ms = report_json.get("duration_ms", 0)
    duration_sec = duration_ms / 1000.0 if duration_ms else 0.0
    bytes_read = report_json.get("bytes_read")
    scanned = report_json.get("scanned", 0)
    throughput_mib = (
        (bytes_read / 1024 / 1024 / duration_sec) if bytes_read and duration_sec else None
    )
    rss_mib = (
        time_v.get("max_rss_kb", 0) / 1024 if time_v.get("max_rss_kb") else None
    )

    def fmt(val, fmt_spec: str = "") -> str:
        if val is None:
            return "n/a"
        if fmt_spec:
            return format(val, fmt_spec)
        return str(val)

    lines = [
        f"# tidymedia 性能采集报告",
        "",
        f"- 时间：`{parse_iso_now()}`",
        f"- 子命令：`{sub}`",
        f"- 数据集：`{data}`",
        f"- 输出目录：`{output_dir}`",
        f"- 退出码：`{return_code}`",
        "",
        "## L1 - Report 概览",
        "",
        "| 指标 | 值 |",
        "|---|---|",
        f"| 扫描文件数 | {scanned} |",
        f"| use case 耗时 | {fmt(duration_ms)} ms ({fmt(duration_sec, '.3f')} s) |",
        f"| 累计读字节 | {fmt(bytes_read)} |",
        f"| 吞吐 | {fmt(throughput_mib, '.2f') if throughput_mib else 'n/a'} MiB/s |",
        "",
        "## L4 - 系统资源（/usr/bin/time -v）",
        "",
        "| 指标 | 值 |",
        "|---|---|",
        f"| Wall clock | {fmt(time_v.get('elapsed_wall'))} |",
        f"| 峰值 RSS | {fmt(rss_mib, '.1f') if rss_mib else 'n/a'} MiB ({fmt(time_v.get('max_rss_kb'))} KB) |",
        f"| User CPU | {fmt(time_v.get('user_time_sec'))} s |",
        f"| System CPU | {fmt(time_v.get('system_time_sec'))} s |",
        f"| CPU 利用率 | {fmt(time_v.get('cpu_percent'))} |",
        f"| 文件系统读 | {fmt(time_v.get('fs_inputs'))} blocks |",
        f"| 文件系统写 | {fmt(time_v.get('fs_outputs'))} blocks |",
        f"| Major page faults | {fmt(time_v.get('major_page_faults'))} |",
        f"| Minor page faults | {fmt(time_v.get('minor_page_faults'))} |",
        f"| 主动上下文切换 | {fmt(time_v.get('vol_ctx_switches'))} |",
        f"| 被动上下文切换 | {fmt(time_v.get('invol_ctx_switches'))} |",
        "",
        "## AI 分析建议提示",
        "",
        "把本报告 + `report.json` + `time-v.txt` 一起扔给 LLM 提问：",
        "",
        "1. 吞吐 vs 峰值 RSS：是否 IO/CPU/内存受限？",
        "2. `User/System CPU` 比例：kernel 时间占比高 → 系统调用密集（open/read 小文件）",
        "3. `Major page faults` 高 → 内存不足开始换页；建议减小 `STREAM_CHUNK` 或增大 RAM",
        "4. `duration_ms` 与 `elapsed_wall` 差 → 后者含 Rust 启动 + tract 加载 ONNX 等固定开销",
        "",
    ]
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(
        description="tidymedia perf-collect: 跑真实数据一次产 AI 可读性能报告",
    )
    ap.add_argument("--sub", required=True, choices=SUBS)
    ap.add_argument("--data", required=True, help="源目录路径")
    ap.add_argument("--output-target", default=None, help="tidymedia -o 目标路径（find 可选）")
    ap.add_argument("--output-dir", required=True, help="perf-collect 产物目录")
    ap.add_argument("--extra", default="", help="附加 tidymedia CLI 参数（空格分隔）")
    ap.add_argument("--project-root", default=".", help="tidymedia 项目根")
    args = ap.parse_args()

    project_root = Path(args.project_root).resolve()
    output_dir = Path(args.output_dir).resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    tidy_bin = find_binary(project_root)
    time_bin = find_gnu_time()
    if time_bin is None:
        print("error: /usr/bin/time not found (Linux) or gtime not installed (macOS)",
              file=sys.stderr)
        return 1

    report_path = output_dir / "report.json"
    time_v_path = output_dir / "time-v.txt"
    extra_args = args.extra.split() if args.extra else []
    cli_args = build_cli(args.sub, args.data, extra_args, report_path, args.output_target)

    env = os.environ.copy()
    env.setdefault("CARGO_PROFILE_RELEASE_OPT_LEVEL", "3")

    print(f"[perf-collect] running: {time_bin} -v {tidy_bin} {' '.join(cli_args)}",
          file=sys.stderr)
    return_code = run_with_time_v(time_bin, tidy_bin, cli_args, time_v_path, env)
    print(f"[perf-collect] tidymedia exit code: {return_code}", file=sys.stderr)

    time_v_data = parse_time_v(time_v_path.read_text(encoding="utf-8", errors="replace"))
    report_data: dict = {}
    if report_path.exists():
        try:
            report_data = json.loads(report_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as e:
            print(f"warn: report.json parse failed: {e}", file=sys.stderr)

    md = render_report(args.sub, args.data, report_data, time_v_data,
                       return_code, output_dir)
    (output_dir / "perf-report.md").write_text(md, encoding="utf-8", newline="\n")
    print(f"[perf-collect] wrote {output_dir / 'perf-report.md'}", file=sys.stderr)
    return 0 if return_code == 0 else return_code


if __name__ == "__main__":
    sys.exit(main())
