# cargo-env.sh
# 声明此文件不应被直接执行，只能被 source 导入
# shellcheck disable=SC2148

# 1. 强一致性锁配置
# 在本地开发若需要自动更新 Lock，可临时将此变量改为空字符串 ""
CARGO_LOCKED="--locked"

# 2. 核心公共目标与特征参数（使用数组定义以确保参数解析的稳健性）
# 与 codex-bot 的差异：skel_rs 的 sqlite/postgres/mysql 三 backend 用 compile_error!() 强制
# 互斥，MUST NOT 用 --all-features；features 经 FEATURES env 注入（默认 http,sqlite）。
CARGO_COMMON_FLAGS=(
  "--features=${FEATURES:-http,sqlite}"
  "--all-targets"     # 覆盖所有目标（lib, bin, tests, benches, examples）
  "--workspace"       # 作用于整个工作空间
  "--release"
)
