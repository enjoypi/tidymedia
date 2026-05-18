plugins {
    id("com.android.application")
    kotlin("android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "com.happyfactory.tidymedia"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.happyfactory.tidymedia"
        minSdk = 30  // Android 11+，Scoped Storage 强制；READ_MEDIA_* 在 33+ 才生效，30-32 用旧 READ_EXTERNAL_STORAGE
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"

        // 只构 aarch64（现代 Android 设备主流）；i686/x86_64 模拟器需要再加。
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
    }

    // build-android.sh 通过 cargo ndk 把 libtidymedia.so 输出到该目录，
    // Gradle 会自动打包进 APK 的 jniLibs/arm64-v8a/。
    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
            // uniffi-bindgen-cli 生成的 Kotlin 文件目录
            java.srcDirs("src/main/java", "../../uniffi-generated")
        }
    }

    buildTypes {
        getByName("release") {
            isMinifyEnabled = false
        }
        getByName("debug") {
            isDebuggable = true
        }
    }
}

dependencies {
    // Compose BOM 控版本一致性
    implementation(platform("androidx.compose:compose-bom:2024.10.00"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.documentfile:documentfile:1.0.1")

    // uniffi 运行时（Kotlin 绑定依赖 net.java.dev.jna）
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    debugImplementation("androidx.compose.ui:ui-tooling")
}
