# Alpha Mobile — Android 壳（Kotlin + Jetpack Compose）

L301「搭建 Android Kotlin + Rust 混合开发环境」交付面：Gradle 工程 + Compose
壳 + alpha-mobile UniFFI 绑定消费面。架构与决策见
`docs/mobile-core-architecture.md`（§10 构建管线、§12 验收边界）。

## 布局

```
mobile/android/
├── gen-bindings.sh            # 绑定生成 + arm64 .so 交叉编译（构建前跑一次）
├── settings.gradle.kts        # :app 单模块
├── build.gradle.kts           # 插件版本集中声明（AGP 8.9.0 / Kotlin 1.9.25，锁版原因见下）
├── gradle/wrapper/            # Gradle 8.11.1（wrapper jar/属性入库）
└── app/
    ├── build.gradle.kts       # minSdk 26 / compileSdk 35 / Compose / abiFilters arm64-v8a
    └── src/main/
        ├── java/com/alpha/finance/mobile/   # 壳层（手写）
        │   ├── MainActivity.kt    # Compose 界面（状态+快照列表+行内分析）
        │   ├── AlphaBridge.kt     # UniFFI MobileCore 消费面（Dispatchers.IO）
        │   └── MarketModels.kt    # FFI JSON 载荷 ↔ kotlinx-serialization 模型
        ├── java/uniffi/alpha_mobile/alpha_mobile.kt   # uniffi 0.25 生成（入库）
        ├── jniLibs/arm64-v8a/     # libalpha_mobile.so（生成，不入库）
        ├── jniLibs/x86_64/        # 模拟器 ABI（gen-bindings.sh 双目标，L517）
        └── ...
```

## 构建步骤（本机已实证，2026-09-30）

```bash
# 0) 前置：rustup、Android SDK(platforms;android-35 + build-tools;35.0.0)、
#    NDK r27+，ANDROID_HOME 指向 SDK；gradle wrapper 已入库，无需预装 Gradle
# 1) 生成 Kotlin 绑定 + 交叉编译 .so 进 jniLibs
mobile/android/gen-bindings.sh
# 2) 打调试包（产物 app/build/outputs/apk/debug/app-debug.apk）
cd mobile/android && ./gradlew :app:assembleDebug
# 3)（可选）JVM 载荷契约单测（无需设备/.so）
./gradlew :app:testDebugUnitTest
```

## 入库约定

- **入库**：Gradle 工程文件、手写 Kotlin、uniffi 生成的 `alpha_mobile.kt`
  （消费面随提交可审；Rust 侧 FFI 面变更后必须重跑 `gen-bindings.sh`——
  运行期有 checksum 校验兜底）。
- **不入库**（`.gitignore`）：`build/`、`local.properties`（`sdk.dir` 本机路径）、
  `jniLibs/**/*.so`、`.gradle/`。

## 版本锁

- **Kotlin 1.9.25 + compose compiler 1.5.15**：uniffi 0.25 生成的 Kotlin 在
  2.x（K2）下过载歧义（`MobileException.Failed` 的构造属性与 override
  `message` 同名双候选），**升级 Kotlin 的前置条件是升级 uniffi**（workspace
  pin 0.25，L118 假设②），两层锁同进退；契约测试有负向断言守卫。
- AGP 8.9.0 ↔ Gradle 8.11.1（wrapper）↔ JDK 17+；JNA 5.13.0（uniffi 生成面
  硬依赖）。

## 边界（真机/发布归后续 TODO）

- 真机加载 `.so` 与运行时验证：本机无设备，模拟器/真机验收归后续 TODO；
  x86_64 ABI 已落地（L517：gen-bindings.sh 双目标 + abiFilters 双列入）。
- 发布流水线（签名/R8 规则/AAB 分包）：TODO「多平台发布流水线」。
- JNI 直接回调（SDK 推送/生命周期）：TODO 272 逃生舱，当前纯 UniFFI 面。
