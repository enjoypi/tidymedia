// 顶层 build 脚本：仅声明 plugin 版本，具体 apply 在 :app 里。
// Android Studio 与 Gradle 8.x / AGP 8.7+ / Kotlin 2.0+ 配合下，
// Compose 编译器随 Kotlin 自带（kotlin("plugin.compose") 替代旧 composeOptions）。
plugins {
    id("com.android.application") version "8.10.0" apply false
    kotlin("android") version "2.0.21" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21" apply false
}
