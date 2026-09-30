// L301：app 模块——Jetpack Compose 壳 + UniFFI 生成绑定（src/main/java/uniffi/
// alpha_mobile/，由 gen-bindings.sh 生成并入库）+ JNA 载入 libalpha_mobile.so
// （.so 由 gen-bindings.sh 交叉编译进 src/main/jniLibs/，不入库）。
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.serialization")
}

android {
    namespace = "com.alpha.finance.mobile"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.alpha.finance.mobile"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            // gen-bindings.sh 目前只产 arm64-v8a（主流真机 + arm64 模拟器镜像）
            abiFilters += listOf("arm64-v8a")
        }
    }

    buildTypes {
        release {
            // 混淆关闭：uniffi 生成面靠 JNA 反射，R8 处理规则归发布流水线 TODO
            isMinifyEnabled = false
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
    composeOptions {
        // 与 Kotlin 1.9.25 官方配对；2.x K2 与 uniffi 0.25 生成面不兼容（见根 build 注释）
        kotlinCompilerExtensionVersion = "1.5.15"
    }
}

dependencies {
    // UniFFI 0.25 生成的绑定用 JNA Native.load("alpha_mobile") 载入库
    //（@aar 变体带 Android 原生装载器；宿主 JVM 单测走 jar 变体）
    implementation("net.java.dev.jna:jna:5.13.0@aar")
    testImplementation("net.java.dev.jna:jna:5.13.0")

    implementation(platform("androidx.compose:compose-bom:2024.12.01"))
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")
    // FFI 载荷 = JSON 字符串（serde 字段契约），Kotlin 侧 kotlinx-serialization 解析
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.6.3")

    testImplementation("junit:junit:4.13.2")
}
