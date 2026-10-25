# Android 工程规则

- Android app 使用 feature `android-app`；Rust FFI 的 Record、Error、CommandResult 和 loader 规则见 `src/CLAUDE.md`，本文件不重复。
- uniffi 0.31 使用 proc-macro 模式：crate 根调用 `uniffi::setup_scaffolding!()`，导出项使用 `#[uniffi::export]`，Record/Error 使用对应 derive。
- 工具链基线：JDK 25 Temurin、Gradle 9.1、AGP 8.10、Kotlin 2.0.21、NDK r26d、SDK android-35。升级时同步验证 uniffi 绑定和 jniLibs 产物。
- `[lib] crate-type` 保持 `rlib`，不在 Cargo.toml 固定 cdylib；交叉编译时按需覆盖，避免 Windows 与 bin 的 PDB 同名冲突。
- 交叉编译统一执行 `mobile/build-android.sh`；aarch64、Android API 30、jniLibs 输出和 cdylib 参数以该脚本为准，不在文档复制命令。

- 该构建属于 justfile 未覆盖的专项操作；`ANDROID_HOME` 和 `ANDROID_NDK_HOME` 只从环境读取。
- Kotlin 绑定使用现有 uniffi-bindgen 从生成的 `libtidymedia.so` 生成，输出到 Android 工程既定绑定目录；生成文件不手工维护并行副本。
- 移动端 UI 必须保留查看失败和人工核对结果的入口，不能把自动整理结果呈现为不可逆保证。
