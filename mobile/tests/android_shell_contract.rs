//! Android 壳接线契约（L301）
//!
//! 为什么需要它：Gradle/Kotlin 骨架**不在 workspace 门禁的编译范围里**（CI 的
//! Linux 作业没有 Android SDK，本机有 SDK 但门禁不该依赖外部工具链），于是
//! 和 desktop 的 wiring_contract 同理——把「混合工程里不该漂移的东西」变成
//! 可本地执行的断言：
//! * Rust↔Gradle 两侧名字一致（cdylib `alpha_mobile` ↔ JNA `Native.load` ↔
//!   uniffi 命名空间 ↔ jniLibs 路径 ↔ gen-bindings.sh 产物位置）；
//! * 生成绑定（入库的 `alpha_mobile.kt`）确实暴露壳层在用的 FFI 面（构造器、
//!   三个 JSON 方法、MobileException 两变体）——改 Rust 侧 FTI 面而不重跑
//!   gen-bindings.sh 会在这里断；
//! * 壳层消费纪律：FFI 只走 AlphaBridge + Dispatchers.IO，Compose 层不直接
//!   import 生成绑定；
//! * 载荷模型覆盖 serde 字段（snake_case 契约逐字段点名）。
//!
//! 不能覆盖的部分（诚实边界）：Kotlin/AGP 是否真编过、APK 是否可装——需
//! Android SDK，本机已实证（README），CI 靠本文件守结构不漂移。

use std::path::{Path, PathBuf};

fn android_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("android")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(android_dir().join(rel))
        .unwrap_or_else(|e| panic!("读 android/{rel} 失败: {e}"))
}

/// 与 desktop/tests/wiring_contract.rs 同款：剥注释后断言（Kotlin/KTS 无块注释嵌套困扰）
fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_contains(haystack: &str, needles: &[&str], what: &str) {
    for needle in needles {
        assert!(
            haystack.contains(needle),
            "{what} 缺关键接线 {needle:?}（骨架被改动？）"
        );
    }
}

/// 工程文件齐全：wrapper 入库（无需预装 Gradle）、单模块 :app
#[test]
fn gradle_project_layout_is_complete() {
    for rel in [
        "settings.gradle.kts",
        "build.gradle.kts",
        "gradle.properties",
        "gradlew",
        "gradle/wrapper/gradle-wrapper.properties",
        "app/build.gradle.kts",
        "app/src/main/AndroidManifest.xml",
        "app/src/main/res/values/strings.xml",
        "app/src/main/res/values/themes.xml",
        ".gitignore",
        "gen-bindings.sh",
        "README.md",
    ] {
        let path = android_dir().join(rel);
        assert!(path.is_file(), "缺工程文件 {rel}");
    }
}

/// Gradle 接线：插件版本组合、模块坐标、SDK 档位、ABI 过滤、JNA 依赖
#[test]
fn gradle_wiring_matches_hybrid_layout() {
    let settings = strip_line_comments(&read("settings.gradle.kts"));
    assert_contains(
        &settings,
        &[
            ":app",
            "google()",
            "mavenCentral()",
            "FAIL_ON_PROJECT_REPOS",
        ],
        "settings.gradle.kts",
    );

    let root = strip_line_comments(&read("build.gradle.kts"));
    assert_contains(
        &root,
        &[
            "com.android.application",
            "org.jetbrains.kotlin.android",
            "org.jetbrains.kotlin.plugin.serialization",
            // Kotlin 锁 1.9.25 的原因（不要「顺手」升级）：
            // uniffi 0.25 生成的 Kotlin 在 2.x K2 下过载歧义（错误类同名属性
            // `message`），升级 Kotlin 须先升级 uniffi—— workspace pin 0.25
            // 是 L118 既定假设，两层锁必须同进退
            "1.9.25",
        ],
        "根 build.gradle.kts",
    );
    assert!(
        !root.contains("org.jetbrains.kotlin.plugin.compose"),
        "Kotlin 1.9 组合不得声明 compose 插件（Kotlin 2 起才有），compose 编译器走 composeOptions"
    );

    let wrapper = read("gradle/wrapper/gradle-wrapper.properties");
    assert!(
        wrapper.contains("gradle-8.11.1"),
        "wrapper 版本漂移（AGP 8.9.0 要求 Gradle ≥ 8.11.1，改动需两端同步）: {wrapper}"
    );

    let app = strip_line_comments(&read("app/build.gradle.kts"));
    assert_contains(
        &app,
        &[
            "com.alpha.finance.mobile", // namespace == applicationId
            "minSdk = 26",              // 与 gen-bindings.sh API_LEVEL 一致
            "compileSdk = 35",
            "arm64-v8a", // gen-bindings.sh 只产此 ABI
            "buildFeatures",
            "compose = true",
            "kotlinCompilerExtensionVersion = \"1.5.15\"", // 与 Kotlin 1.9.25 官方配对
            "net.java.dev.jna:jna",                        // uniffi 0.25 生成面走 JNA
            "@aar",
            "kotlinx-serialization-json", // FFI JSON 载荷解析
        ],
        "app/build.gradle.kts",
    );
}

/// Rust↔Gradle↔生成绑定四名一致：cdylib 文件名、JNA 库名、uniffi 命名空间、
/// jniLibs 路径、绑定生成脚本与消费路径
#[test]
fn cross_language_names_stay_consistent() {
    // Rust 侧：crate 名 alpha-mobile → cdylib libalpha_mobile.so（三型含 cdylib）
    let mobile_toml = read("../Cargo.toml");
    assert!(
        mobile_toml.contains("cdylib"),
        "alpha-mobile 必须产出 cdylib（Android .so 的来源）"
    );
    // 绑定 CLI 按 feature 裁剪（默认构建零增量，门禁不受影响）
    assert_contains(
        &mobile_toml,
        &["bindgen", "uniffi/cli", "uniffi-bindgen"],
        "mobile/Cargo.toml",
    );

    // 生成绑定侧：uniffi 命名空间 = alpha_mobile，JNA 载名 = alpha_mobile
    let binding = read("app/src/main/java/uniffi/alpha_mobile/alpha_mobile.kt");
    assert_contains(
        &binding,
        &[
            "package uniffi.alpha_mobile",
            "componentName = \"alpha_mobile\"",
        ],
        "生成的 alpha_mobile.kt",
    );

    // 生成脚本侧：产物两端落位 + NDK linker 档位（API_LEVEL == minSdk）
    let gen = strip_line_comments(&read("gen-bindings.sh"));
    assert_contains(
        &gen,
        &[
            "-p alpha-mobile",
            "--language kotlin",
            "aarch64-linux-android",
            "API_LEVEL=26",
            "jniLibs/arm64-v8a",
            "libalpha_mobile.so",
        ],
        "gen-bindings.sh",
    );

    // 消费侧：壳层 import 的包与生成绑定的 package 一致
    let bridge = read("app/src/main/java/com/alpha/finance/mobile/AlphaBridge.kt");
    assert!(
        bridge.contains("import uniffi.alpha_mobile.MobileCore"),
        "壳层必须消费生成绑定（package uniffi.alpha_mobile），不得自写 JNI 面"
    );
}

/// 生成绑定暴露壳层在用的 FFI 面——改 Rust FFI 面不重跑 gen-bindings.sh 在此断
#[test]
fn generated_bindings_expose_consumed_surface() {
    let binding = read("app/src/main/java/uniffi/alpha_mobile/alpha_mobile.kt");
    assert_contains(
        &binding,
        &[
            "class MobileCore(",
            "fun `quoteJson`(`symbol`: String): String",
            "fun `analyzeJson`(`symbol`: String): String",
            "fun `statusJson`(): String",
            // L337 推送/同步六方法（只增不改：L118 三方法与构造器原样）
            "fun `setAlertRulesJson`(`rulesJson`: String): ULong",
            "fun `checkAlertsJson`(): String",
            "fun `takePendingJson`(): String",
            "fun `syncStatusJson`(): String",
            "fun `syncPlanJson`(`trigger`: String): String",
            "fun `markSyncedJson`(): String",
            "sealed class MobileException",
            "class InvalidSymbol(",
            "class Failed(",
        ],
        "生成绑定 FFI 面",
    );
}

/// 壳层消费纪律：FFI 只走 AlphaBridge + 后台调度；全壳层只有 AlphaBridge 允许
/// import 生成绑定（错误面就地翻译成 Kotlin 类型，UI 层零 uniffi 引用）
#[test]
fn shell_layers_stay_in_their_lanes() {
    let bridge = read("app/src/main/java/com/alpha/finance/mobile/AlphaBridge.kt");
    assert_contains(
        &bridge,
        &[
            "Dispatchers.IO", // 线程模型 §6：主线程禁止直调
            "core.statusJson()",
            "core.quoteJson(",
            "core.analyzeJson(",
            // L337 六透传（决策在 Rust，壳层只搬运 JSON↔载荷）
            "core.setAlertRulesJson(",
            "core.checkAlertsJson()",
            "core.takePendingJson()",
            "core.syncStatusJson()",
            "core.syncPlanJson(",
            "core.markSyncedJson()",
            "MobileException",               // 翻译源：生成绑定异常
            "Error.InvalidSymbol(e.symbol)", // 类型搬运，不加逻辑
            "Error.Failed(",
            "core.destroy()", // Rust Arc 释放面必须接线
        ],
        "AlphaBridge.kt",
    );

    // import uniffi.* 是 AlphaBridge 的特权：壳层其余 .kt 一律不得直接依赖生成绑定
    let shell_dir = android_dir().join("app/src/main/java/com/alpha/finance/mobile");
    for entry in std::fs::read_dir(&shell_dir).expect("读壳层源码目录") {
        let path = entry.expect("读目录项").path();
        if path.extension().and_then(|e| e.to_str()) != Some("kt") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("读 .kt");
        let file = path.file_name().unwrap().to_string_lossy();
        for line in source.lines() {
            assert!(
                !(line.trim_start().starts_with("import uniffi.") && file != "AlphaBridge.kt"),
                "{file} 不得直接依赖生成绑定（仅 AlphaBridge.kt 消费 uniffi），违规行: {line}"
            );
        }
    }

    // Compose 层：接线面 + Kotlin 侧错误面（AlphaBridge.Error，非 MobileException）
    let activity = read("app/src/main/java/com/alpha/finance/mobile/MainActivity.kt");
    assert_contains(
        &activity,
        &[
            "setContent", // Compose 接线
            "MaterialTheme",
            "LazyColumn",
            "LaunchedEffect",
            "AlphaBridge.Error", // 错误转文案不崩壳
            "bridge.close()",    // 生命周期接线：销毁释放 Rust 侧内存
        ],
        "MainActivity.kt",
    );
}

/// 清单↔主题↔Gradle 三方一致：入口 Activity、启动器 intent、主题样式存在
#[test]
fn manifest_theme_and_gradle_agree() {
    let manifest = read("app/src/main/AndroidManifest.xml");
    assert_contains(
        &manifest,
        &[
            ".MainActivity",
            "android.intent.action.MAIN",
            "android.intent.category.LAUNCHER",
            "android:exported=\"true\"",
            "@style/Theme.AlphaMobile",
            "@string/app_name",
        ],
        "AndroidManifest.xml",
    );

    let themes = read("app/src/main/res/values/themes.xml");
    assert!(
        themes.contains("Theme.AlphaMobile"),
        "清单引用的主题必须真实存在"
    );
    let strings = read("app/src/main/res/values/strings.xml");
    assert!(
        strings.contains("app_name"),
        "清单引用的 app_name 必须真实存在"
    );
}

/// 载荷模型逐字段点名 serde 契约（snake_case；与 mobile/src/state.rs 单测同口径）
#[test]
fn payload_models_cover_serde_field_contract() {
    let models = read("app/src/main/java/com/alpha/finance/mobile/MarketModels.kt");
    // MarketData
    assert_contains(
        &models,
        &[
            "QuotePayload",
            "symbol",
            "timestamp",
            "price",
            "volume",
            "bid",
            "ask",
            "open",
            "high",
            "low",
        ],
        "QuotePayload",
    );
    // IndicatorResult + RiskMetrics + AnalysisResult
    assert_contains(
        &models,
        &[
            "IndicatorPayload",
            "name",
            "timestamps",
            "values",
            "signals",
            "RiskMetricsPayload",
            "volatility",
            "sharpe_ratio",
            "max_drawdown",
            "beta",
            "AnalysisPayload",
            "analyzed_at",
            "indicators",
            "risk_metrics",
            "recommendation",
            "confidence",
        ],
        "AnalysisPayload/RiskMetricsPayload",
    );
    // status_json
    assert_contains(
        &models,
        &["StatusPayload", "version", "symbols", "api_url"],
        "StatusPayload",
    );

    // L337 推送/同步载荷（docs/mobile-push-sync.md §4；serde snake_case 契约）
    assert_contains(
        &models,
        &[
            "AlertRulePayload",
            "target_price",
            "NotificationSpecPayload",
            "created_at",
            "AlertsReportPayload",
            "TakenPayload",
            "SyncStatusPayload",
            "last_sync",
            "fingerprint",
            "interval_secs",
            "SyncPlanPayload",
            "since_fingerprint",
            "reason",
        ],
        "推送/同步载荷",
    );

    // JVM 契约单测在（SDK 侧可跑，CI 无 SDK 靠本文件守字段）
    let test = read("app/src/test/java/com/alpha/finance/mobile/PayloadParsingTest.kt");
    assert_contains(
        &test,
        &[
            "QuotePayload",
            "AnalysisPayload",
            "StatusPayload",
            "NotificationSpecPayload",
            "SyncPlanPayload",
            "@Test",
        ],
        "PayloadParsingTest.kt",
    );
}

/// L337 推送/同步接缝：接口面在壳层（PushSyncSeam），决策回路只经 AlphaBridge
/// 六透传；触发串与 Rust serde 契约逐字一致；WorkManager 依赖与 15min 钳制口径在
#[test]
fn push_sync_seam_stays_shell_side() {
    let seam = read("app/src/main/java/com/alpha/finance/mobile/PushSyncSeam.kt");
    assert_contains(
        &seam,
        &[
            "interface NotificationDispatcher", // 送达口（平台实现归真机 TODO）
            "class PeriodicSyncWorker",         // 周期同步 Worker 骨架
            "MIN_PERIODIC_MINUTES",             // WorkManager ≥15min 钳制口径
        ],
        "PushSyncSeam.kt",
    );

    // 触发串跨语言逐字一致（serde snake_case + foreground 显式 rename）
    let bridge = read("app/src/main/java/com/alpha/finance/mobile/AlphaBridge.kt");
    assert_contains(
        &bridge,
        &[
            "TRIGGER_PERIODIC = \"periodic\"",
            "TRIGGER_FOREGROUND = \"foreground\"",
            "TRIGGER_CONNECTIVITY_RESTORED = \"connectivity_restored\"",
            "TRIGGER_MANUAL = \"manual\"",
        ],
        "AlphaBridge 触发串",
    );
    let sync_rs = read("../src/sync.rs");
    assert_contains(
        &sync_rs,
        &[
            "rename = \"foreground\"", // 显式 rename（非 snake_case 推导）
        ],
        "Rust SyncTrigger 线上串",
    );

    // WorkManager 依赖已入壳工程（周期任务装配可编译）
    let gradle = read("app/build.gradle.kts");
    assert_contains(
        &gradle,
        &["androidx.work:work-runtime-ktx"],
        "app/build.gradle.kts",
    );

    // 决策不外溢：PushSyncSeam 不 import uniffi（决策面只在 Rust + AlphaBridge）
    let seam_kt = strip_line_comments(&seam);
    assert!(
        !seam_kt.contains("uniffi."),
        "PushSyncSeam.kt 不得触碰生成绑定（决策面在 Rust 侧）"
    );
}

/// 构建产物不入库；生成绑定入库（消费面随提交可审）
#[test]
fn gitignore_separates_generated_source_from_artifacts() {
    let gitignore = read(".gitignore");
    for pattern in ["build/", "local.properties", "jniLibs/**/*.so", ".gradle/"] {
        assert!(
            gitignore.contains(pattern),
            ".gitignore 缺 {pattern:?}——.so/构建产物会被误提交"
        );
    }
    // 生成绑定本体在库内（契约测试消费的就是它）
    assert!(
        android_dir()
            .join("app/src/main/java/uniffi/alpha_mobile/alpha_mobile.kt")
            .is_file(),
        "uniffi 生成的 alpha_mobile.kt 应入库（.so 才是构建产物）"
    );
}

/// L301 顺带修复的回归守门：alpha-core 的 android JNI 错误映射必须连依赖一起
/// 门控（此前只有 cfg 没有依赖，android target 一编即 E0433 的潜伏孤儿）
#[test]
fn alpha_core_android_jni_mapping_has_its_dependency() {
    let core_toml = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../packages/core/Cargo.toml"),
    )
    .expect("读 packages/core/Cargo.toml");
    let section = core_toml
        .split("[target.'cfg(target_os = \"android\")'.dependencies]")
        .nth(1)
        .expect(
            "alpha-core 缺 android target 依赖段（errors.rs 的 From<jni::errors::Error> 会失联）",
        );
    assert!(
        section.contains("jni = { workspace = true }"),
        "android 段必须挂 jni 依赖（workspace pin 0.21）"
    );
}
