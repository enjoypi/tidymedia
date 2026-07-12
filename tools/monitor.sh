#!/usr/bin/env bash
# skel_rs server-debug 监控命令复用（/server-debug skill 入口）
# 用法:
#   tools/monitor.sh crash      # server 崩溃事件（panic/stack overflow/SIGSEGV，靠 RUST_BACKTRACE=full 自动留栈）
#   tools/monitor.sh business   # 业务错误/警告（result=error / WARN / ERROR）
# 与会话内 Monitor task 的命令一致，便于人工 bash 调试复用。日志路径默认 scratchpad/skel_rs.log，
# 可用 SERVICE_LOG 覆盖。
set -euo pipefail

cd "$(dirname "$0")/.."
LOG="${SERVICE_LOG:-scratchpad/skel_rs.log}"

case "${1:-}" in
    crash)
        # launch 注入的 RUST_BACKTRACE=full 自动把 backtrace 打到日志
        tail -F "$LOG" 2>&1 \
            | grep -E --line-buffered 'panicked at|stack backtrace|fatal runtime error|stack overflow|SIGABRT|SIGSEGV|thread .* panicked'
        ;;
    business)
        tail -F "$LOG" 2>&1 \
            | grep -E --line-buffered '"result":"(error|http_error|api_error|decode_error|missing)"|"level":"(WARN|ERROR)"|panicked at'
        ;;
    *)
        echo "用法: $0 {crash|business}" >&2
        exit 1
        ;;
esac
