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
            // gen-bindings.sh 双 ABI（L517）：arm64-v8a 真机 + x86_64 模拟器/Chromebook
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    // L517 分包：APK 侧按 ABI 拆分 + universal 兜底（直发渠道按机型自选或全装）；
    // Play 渠道走 AAB——设备维度的分包由 Play 动态下发，splits 对 AAB 不生效
    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "x86_64")
            isUniversalApk = true
        }
    }

    // L517 多渠道：play = AAB 上架 Google Play；direct = 官网/侧载 universal APK。
    // applicationId 保持一致（同一应用身份），渠道以 BuildConfig 标记供运行期
    // 与更新检查区分（docs/android-release.md §渠道矩阵）
    flavorDimensions += "channel"
    productFlavors {
        create("play") {
            dimension = "channel"
            buildConfigField("String", "DISTRIBUTION_CHANNEL", "\"play\"")
        }
        create("direct") {
            dimension = "channel"
            buildConfigField("String", "DISTRIBUTION_CHANNEL", "\"direct\"")
        }
    }

    signingConfigs {
        create("release") {
            // 密钥全走环境变量/CI secret，keystore 绝不入库（docs/android-release.md §签名）
            System.getenv("ALPHA_KEYSTORE_PATH")?.let { storeFile = file(it) }
            storePassword = System.getenv("ALPHA_KEYSTORE_PASSWORD")
            keyAlias = System.getenv("ALPHA_KEY_ALIAS")
            keyPassword = System.getenv("ALPHA_KEY_PASSWORD")
        }
    }

    buildTypes {
        release {
            // 混淆关闭：uniffi 生成面靠 JNA 反射，R8 处理规则归发布流水线 TODO
            isMinifyEnabled = false
            // keystore 环境未配置时保持 unsigned（CI/本机冒烟可构建，不可安装）
            if (System.getenv("ALPHA_KEYSTORE_PATH") != null) {
                signingConfig = signingConfigs.getByName("release")
            }
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
        buildConfig = true // 渠道标记 DISTRIBUTION_CHANNEL（L517 多渠道）
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
    // L337：周期后台同步（WorkManager 系统钳制 ≥15min，见 PushSyncSeam 注释）
    implementation("androidx.work:work-runtime-ktx:2.9.1")
    // FFI 载荷 = JSON 字符串（serde 字段契约），Kotlin 侧 kotlinx-serialization 解析
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.6.3")

    testImplementation("junit:junit:4.13.2")
}
