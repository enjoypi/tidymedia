# tidymedia justfile - build / nextest / llvm-cov 统一入口
#
# 日常 build/test/lint/cov 使用 nightly + dev；普通 build/test 保持 default features，
# lint/cov 使用 all-features。Windows 的编译型命令经 tools/msvc-env.cmd 导入 MSVC 环境。
# 普通 target/ 与 coverage 的 target/llvm-cov-target/ 保持隔离，各自增量复用。
#
# 用法：
#   just build            日常 debug 构建（nightly + dev）
#   just build-release    发布/性能/真跑构建（release opt=3）
#   just OPT=0 build-release  快速 release 编译验证
#   just test             nextest 全量（default features）
#   just test xavc        nextest 按名字子串过滤
#   just cov              覆盖率门禁（Linux CI 权威；Windows 可运行）
#   just gate             fmt -> lint -> cov -> build（cov 已含全量 nextest）
#   just lint / fmt / clean

set shell := ["bash", "-cu"]

# 仅显式 release 构建使用；不影响日常 dev 产物指纹
OPT := "3"

# Windows 下经 cmd 调 msvc-env.cmd 设环境后执行；unix 直接跑
CMD := 'C:\Windows\System32\cmd.exe'

default:
    @just --list

# cov 已执行全量 nextest；完整门禁不再重复普通 test。coverage MUST 独占运行。
gate: fmt lint cov build

# === 构建 ===
[windows]
build:
    "{{ CMD }}" //c "call tools\msvc-env.cmd && cargo +nightly build --locked"

[unix]
build:
    cargo +nightly build --locked

[windows]
build-release:
    "{{ CMD }}" //c "call tools\msvc-env.cmd && set \"CARGO_PROFILE_RELEASE_OPT_LEVEL={{ OPT }}\" && cargo build --release --locked"

[unix]
build-release:
    CARGO_PROFILE_RELEASE_OPT_LEVEL={{ OPT }} cargo build --release --locked

# === 测试（nextest）===
# just test            全量（lib + 集成，default features）
# just test <name>     按名字子串过滤，如 just test xavc
[windows]
test *ARGS:
    "{{ CMD }}" //c "call tools\msvc-env.cmd && cargo +nightly nextest run --locked --no-fail-fast {{ ARGS }}"

[unix]
test *ARGS:
    cargo +nightly nextest run --locked --no-fail-fast {{ ARGS }}

# === 覆盖率（llvm-cov）===
# 唯一门禁实现见 tools/cov.sh；顺序 = lcov BRDA branch 100%
# -> report region 100%（function/line 同门禁）-> DA 真值 plate + 生产文件完整性自检。
[linux]
cov:
    bash tools/cov.sh

[windows]
cov:
    "{{ CMD }}" //c "call tools\msvc-env.cmd && bash tools/cov.sh"

[macos]
cov:
    @echo "cov 完整门禁仅 Linux CI 权威；macOS 已知平台缺口，本机可跑 just test" >&2
    @exit 1

# === fmt / lint ===
fmt:
    cargo +nightly fmt --all

[windows]
lint:
    "{{ CMD }}" //c "call tools\msvc-env.cmd && cargo +nightly clippy --all-targets --all-features --locked --no-deps -- -D warnings"

[unix]
lint:
    cargo +nightly clippy --all-targets --all-features --locked --no-deps -- -D warnings

# === 清理 ===
clean:
    cargo clean
