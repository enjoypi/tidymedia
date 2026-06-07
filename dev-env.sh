#!/usr/bin/env bash
# Windows + clean-bash hook（env 被剥到仅 HOME/PATH/PWD）下的开发环境补全。
# 用法（shell 状态不跨命令保留，须在同一条命令内 source）：
#   source ./dev-env.sh && cargo nextest run --release
#
# WHY 每组变量：
# - PATH：cargo/rustup 在 ~/.cargo/bin，不在精简后的 PATH 里
# - TMP/TEMP：缺失时 Rust temp_dir() 回退 C:\WINDOWS → tempfile PermissionDenied
# - SYSTEMROOT/WINDIR/ProgramFiles*/ProgramData：rustc 探测 MSVC link.exe 依赖；
#   缺失时回退到 PATH 上 Git bash 的 GNU link.exe（coreutils）→ 链接失败
export PATH="$HOME/.cargo/bin:$PATH"

_win_home="$(cygpath -w "$HOME" 2>/dev/null || echo "C:\\Users\\${HOME##*/}")"
export TMP="${_win_home}\\AppData\\Local\\Temp"
export TEMP="$TMP"
export SYSTEMROOT='C:\Windows'
export WINDIR='C:\Windows'
export ProgramFiles='C:\Program Files'
export ProgramData='C:\ProgramData'

# bash 不允许 export 名字含括号的变量，而 MSVC 探测要读 ProgramFiles(x86)
# → 用 wenv 注入；cargo 函数包一层让日常用法无感
wenv() { env 'ProgramFiles(x86)=C:\Program Files (x86)' "$@"; }
cargo() { wenv "$HOME/.cargo/bin/cargo" "$@"; }
