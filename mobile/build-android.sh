#!/usr/bin/env bash
# 把 Rust core 交叉编译到 Android（aarch64），生成 libtidymedia.so，
# 然后用 uniffi-bindgen-cli 从 .so 二进制 metadata 提取生成 Kotlin 绑定。
#
# 先决条件：
# - 安装 Android NDK r26+（设 ANDROID_NDK_HOME 或 ANDROID_NDK_ROOT）
# - `cargo install cargo-ndk`（自动检测 NDK）
# - `cargo install uniffi --features cli`（提供 `uniffi-bindgen` binary）
#
# 输出：
# - mobile/android/app/src/main/jniLibs/arm64-v8a/libtidymedia.so
# - mobile/uniffi-generated/uniffi/tidymedia/tidymedia.kt（uniffi 默认包名 uniffi.<crate>）
set -euo pipefail

# 脚本所在目录
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"
REPO_ROOT="$( cd "$SCRIPT_DIR/.." >/dev/null 2>&1 && pwd )"

cd "$REPO_ROOT"

# 1. 交叉编译 cdylib，目标 aarch64-linux-android，platform API 30（与 minSdk 对齐）。
#    Cargo.toml 的 crate-type 只声明 rlib（cdylib 写死会让 Windows 上 lib/bin
#    的 tidymedia.pdb 同名冲突，cargo#6313），这里用 `rustc --crate-type` 按需覆盖。
echo ">>> [1/3] cargo ndk: target aarch64-linux-android, API 30, features=android-app"
cargo ndk \
    --target aarch64-linux-android \
    --platform 30 \
    --output-dir "$SCRIPT_DIR/android/app/src/main/jniLibs" \
    rustc --lib --crate-type cdylib --release --features android-app

SO_PATH="target/aarch64-linux-android/release/libtidymedia.so"
if [[ ! -f "$SO_PATH" ]]; then
    echo "error: $SO_PATH not built" >&2
    exit 1
fi

# 2. 用 uniffi-bindgen 从 .so metadata 抽取生成 Kotlin 绑定
echo ">>> [2/3] uniffi-bindgen generate Kotlin"
mkdir -p "$SCRIPT_DIR/uniffi-generated"
uniffi-bindgen generate \
    --library "$SO_PATH" \
    --language kotlin \
    --out-dir "$SCRIPT_DIR/uniffi-generated"

# 3. 提示后续步骤
echo ">>> [3/3] done"
echo "  .so: $SCRIPT_DIR/android/app/src/main/jniLibs/arm64-v8a/libtidymedia.so"
echo "  kt : $SCRIPT_DIR/uniffi-generated/uniffi/tidymedia/tidymedia.kt"
echo
echo "Next:"
echo "  cd mobile/android && ./gradlew installDebug      # 真机/模拟器安装"
echo "  adb shell am start -n com.happyfactory.tidymedia/.MainActivity"
