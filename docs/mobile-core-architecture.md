# 移动端 Rust 核心库架构（JNI + UniFFI）

> TODO.md「📱 移动端应用」节第 1 项「设计移动端 Rust 核心库架构（JNI + UniFFI）」
> 的落地说明（台账编号沿用 L112–L117 的 dispatch 顺序假设，本文记为 **L118**；
> 骨架 crate = `mobile/` → `alpha-mobile`）。文末登记非交互假设与真机验收边界。

## 1. 目标与口径

* **目标**：一份 Rust 核心（业务与计算在 `alpha-core`，移动端只加状态/边界/演示
  数据源），一条 FFI 桥接源（UniFFI），Kotlin（JNI 侧）与 Swift 两端从同一份
  Rust 定义生成绑定；错误、数据契约、线程模型先在文档与单测里定死。
* **本轮交付**：设计文档（本文件）+ 最小可编译骨架（`alpha-mobile`，进 workspace
  成员名单因而随 lint/test 门禁走）+ 框架层单测（Linux 全绿）。
* **L301 增量交付**（TODO 270）：Android Kotlin + Jetpack Compose 混合工程
  （`mobile/android/`，§9/§10.1）——绑定生成、arm64 交叉编译、Gradle
  assembleDebug 与 JVM 契约单测均在本机实证；真机/模拟器**运行**仍属边界
  （§12①）。
* **仍不交付**（按 TODO 顺序留给后续项，见 §13）：Swift/SwiftUI 集成（271）、
  推送/后台同步（272）、离线存储与同步（274）、触屏交互（273）。iOS 侧无
  Xcode 不可执行；Android 门禁化验证靠契约单测（CI 无 SDK 也能守结构）。

## 2. 架构总览

```
┌──────────── 平台壳（后续 TODO：270/271）────────────┐
│  Kotlin/Compose 业务        Swift/SwiftUI 业务         │
│        │ JNI（uniffi 生成的绑定 + 手写 JNI 逃生舱）        │
│        │        Swift（uniffi 生成的绑定）                │
└────────┼───────────────────────┼──────────────────────┘
         ▼                       ▼
┌──────────── FFI 桥（本轮骨架：alpha-mobile）───────────┐
│  uniffi::setup_scaffolding! + #[uniffi::export]        │
│  MobileCore（观察列表/配置/演示数据源/错误映射）            │
│  载荷 = JSON 字符串（quote_json/analyze_json/status_json）│
└────────┬──────────────────────────────────────────────┘
         ▼
┌──────────── 共享核心（已落地）─────────────────────────┐
│  alpha-core：AnalysisEngine（指标/风险/推荐）、models、    │
│  errors（AlphaError）——与 web/desktop 同一份计算           │
└───────────────────────────────────────────────────────┘
```

依赖方向与 docs/cross-platform-architecture.md §3 一致：平台壳 → FFI 桥 → 共享
核心，反向（核心引用平台类型）禁止。`alpha-mobile` 不引入任何平台 SDK 依赖，
所以能在纯 Linux CI 编译、测试——这是把它放进 workspace `members`（原 exclude
名单移除）的前提，也是骨架可验证性的来源。

## 3. JNI 与 UniFFI 的分工决策

| 方案 | 角色 | 决策 |
|---|---|---|
| **UniFFI**（`uniffi = 0.25`，workspace 已 pin） | 单一桥接源：Rust 定义（proc-macro，无 UDL 副本）→ 自动生成 Kotlin/Swift 绑定 | ✅ 骨架实现这条路径（编译期验证） |
| **裸 JNI**（`jni = 0.21`，workspace 已 pin） | 逃生舱：uniffi 覆盖不到的场景——SDK 级回调（推送 token、生命周期）、性能敏感的零拷贝通道 | ⏸ 本轮不引用；架构上预留 pin 与本文档，首个真实场景（TODO 270/272）落地时启用 |

理由：两端绑定同源（一份 Rust 定义，消除 Kotlin/Swift 双份手写 FFI 声明漂移）；
UniFFI 的 Kotlin 绑定底层走 JNI，纯 Rust 侧无需 JDK 即可编译验证；裸 JNI 只在
「回调进 JVM」方向是刚需，那是平台壳的事，不该出现在核心库骨架里。假设 ④（文末）。

## 4. FFI 数据契约：JSON 字符串桥

骨架的 FFI 载荷是 **JSON 字符串**（`quote_json` / `analyze_json` / `status_json`）
而非 `uniffi::Record`：

* `AnalysisResult`/`MarketData` 定义在 `alpha-core`，无法跨 crate 派生
  `uniffi::Record`（derive 需在定义处）；用 Record 就得在桥层手工重声明整棵
  嵌套 DTO 树（indicators/values/risk_metrics…），与 web/desktop 已有 JSON 契约
  形成第三份结构副本。
* JSON 让三端（web、desktop、mobile）对同一 Rust 类型走**同一序列化口径**
  （serde 字段名即契约），单测可直接断言载荷字段。
* 演进路径：载荷形态收敛稳定后，热点小对象可单独加 `uniffi::Record` 导出——
  proc-macro 模式允许混合（同一 crate 内两种导出并存），替换是增量的。

字段契约（由 `state.rs` 单测锁定）：`quote_json` = `MarketData` serde 字段
（symbol/timestamp/price/volume/bid/ask/open/high/low）；`analyze_json` =
`AnalysisResult` serde 字段（symbol/indicators/recommendation/confidence/
risk_metrics）；`status_json` = `{version, symbols, api_url}`。

## 5. 错误映射

```
AlphaError（alpha-core，含 InvalidInput/CalculationError/JniError 等 15 变体）
    │ From<AlphaError>（桥层，只翻译不吞错）
    ▼
MobileError（uniffi::Error + thiserror，FFI 上是「抛异常」）
    ├─ InvalidSymbol { symbol }   —— 观察列表外的标的（核心库自己的业务判断）
    └─ Failed { detail }          —— 其余核心错误（Display 全文透传）
```

Kotlin/Swift 侧收到的是类型化异常（`MobileError.InvalidSymbol` 等），不是错误码
字符串——错误分类在 Rust 侧判定，平台壳只负责展示。`message` 用 `Display` 全文：
宁可长也不猜（与桌面端「降级要如实」同口径）。

## 6. 线程模型

* `AnalysisEngine::analyze_symbol` 是 `async fn` 但**函数体零 await**（纯计算），
  骨架在 `MobileCore::analyze` 里用 **current-thread tokio 运行时按调用驱动**
  （每次调用建/销运行时），`MobileCore` 本体保持纯数据（`Send + Sync`，可被
  uniffi 跨线程共享）。
* 平台壳侧的义务（骨架只声明、无法在此验证）：**主线程禁止直调** FFI 方法，
  Kotlin 侧放 `Dispatchers.Default`/协程，Swift 侧放后台队列——重活上后台是
  平台壳约定，见真机边界 ③。
* 高频调用时每次新建运行时有开销：真机优化项（复用 `Runtime`）留注释，不提前做
  （假设 ⑥）。

## 7. 观察列表：核心库唯一的业务判断

`MobileCore` 持 `symbols`（观察列表）与 `api_url`（后端地址配置槽，TODO 272/274
的同步目标）。`quote`/`analyze` **只接受观察列表内的标的**，否则
`Err(InvalidSymbol)`——移动端能力边界在核心库收口（有单测），不让平台壳各判一遍。
`api_url` 骨架内只随 `status_json` 下行（探测/同步属 TODO 274，本轮不联网）。

## 8. 演示数据源

`market.rs` = 与桌面端 `desktop/src/market.rs` **同口径的确定性占位行情**
（symbol 派生 FNV 种子 + LCG 几何游走，价格与时间无关、可断言；`synthetic_quote`
/`synthetic_series` 两个入口）。真实数据源接入（api-gateway）是后续 TODO，届时
只换本模块取数实现，`MobileCore` 与 FFI 不动。骨架期复制而非共享模块的原因：
桌面/移动两端生命周期不同，过早上提到 `alpha-core` 会把「演示占位」焊进共享层
（§5 依赖方向的反向搬运禁忌）；两端收敛到共享层等真实数据源接入时一并做。

## 9. 构建管线（Android 侧已实证，L301；iOS 侧仍属设计）

| 平台 | 产物 | 工具链 | 状态 |
|---|---|---|---|
| Android | `cdylib`（`libalpha_mobile.so`）+ Kotlin 绑定 | Rust + NDK clang linker + Gradle（wrapper 8.11.1 / AGP 8.9.0 / Kotlin 1.9.25） | **本机 assembleDebug 实证通过（L301）** |
| iOS | `staticlib`（`.a`，Swift 侧经 uniffi 生成的 Swift 绑定链接） | Xcode + Swift Package（TODO 271） | 设计，无工具链 |

Android 实际管线（`mobile/android/gen-bindings.sh`，两步都不需要 cargo-ndk——
单 target 用 `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER` 指到 NDK 的
`aarch64-linux-android26-clang` 即可，minSdk 26 对齐）：

1. **绑定生成**：`cargo run -p alpha-mobile --features bindgen --bin
   uniffi-bindgen -- generate --library target/debug/libalpha_mobile.so
   --language kotlin --out-dir mobile/android/app/src/main/java`。生成的
   `uniffi/alpha_mobile/alpha_mobile.kt` **入库**（消费面随提交可审；FFI 面
   变更必须重跑，运行期 checksum 兜底）。壳侧依赖 JNA（`jna@aar`，uniffi 0.25
   生成面用 `Native.load("alpha_mobile")`）。
2. **交叉编译**：`cargo build -p alpha-mobile --target aarch64-linux-android`
   → `.so` 拷进 `app/src/main/jniLibs/arm64-v8a/`（**不入库**，`.gitignore`）。

`setup_scaffolding!`（proc-macro-only，无 UDL 副本——避免「UDL 与 Rust 签名
漂移」这一 uniffi 头号事故源）。**L301 顺带修复**：`packages/core` 的
`From<jni::errors::Error>` 此前只有 `#[cfg(target_os = "android")]` 门控而无
对应依赖（android target 一编即 E0433 的潜伏孤儿，从未参与编译所以从未暴露），
按同文件 js-sys 先例补 `[target.'cfg(target_os = "android")'.dependencies]`。

## 10. 骨架 API 与单测

```
mobile/src/lib.rs     crate 文档 + uniffi::setup_scaffolding! + 重导出
mobile/src/state.rs   MobileCore / MobileError / FFI 导出（+ 单测 ~11 例）
mobile/src/market.rs  确定性演示行情（+ 单测 ~5 例）
```

Rust 层 API（单测可直接调）：`new(symbols, api_url)`、`symbols()`、`api_url()`、
`quote(symbol)`、`analyze(symbol)`；FFI 层（`#[uniffi::export]`）：构造器 +
`quote_json` / `analyze_json` / `status_json`。单测覆盖：构造与 getter、观察列表
内/外的 quote 与 analyze（InvalidSymbol 分支）、行情确定性（同种子同序列）、
JSON 载荷字段契约、`From<AlphaError>` 映射、空观察列表退化。

覆盖边界（诚实声明）：**FFI 层只到编译**——`setup_scaffolding!` 与 `export`
宏展开由门禁（`cargo clippy/test --workspace`）验证；绑定的**生成**已在 L301
实证（见 §9），Kotlin 壳在本机 Gradle 下 assembleDebug 通过；仍不可观察的
部分（真机加载 .so、ANR 表现、压测）见 §12。

### 10.1 Android 壳（L301 交付面，`mobile/android/`）

```
mobile/android/
├── gen-bindings.sh            绑定生成 + arm64 .so 交叉编译（§9 两步的脚本化）
├── settings/build.gradle.kts  单模块 :app；AGP 8.9.0 + Kotlin 1.9.25（composeOptions 1.5.15 + serialization 插件）
├── gradle/wrapper/            Gradle 8.11.1（wrapper jar/属性入库）
└── app/src/
    ├── main/java/com/alpha/finance/mobile/
    │   ├── MainActivity.kt    Compose 界面（状态头 + 快照 LazyColumn + 行内分析；L367 挂三手势 modifier）
    │   ├── AlphaBridge.kt     唯一 uniffi 消费点（Dispatchers.IO + MobileException→Error 翻译；L337 增推送/同步六透传与 TRIGGER_* 常量；L367 实现 RefreshGateway）
    │   ├── Gestures.kt        触屏手势契约与刷新编排（L367：GestureAction/targetBridgeMethod/refreshWithManualSync，纯逻辑零 Compose/uniffi 依赖）
    │   ├── MarketModels.kt    FFI JSON 载荷 ↔ kotlinx-serialization（@SerialName 对齐 serde；L337 增推送/同步六载荷）
    │   └── PushSyncSeam.kt    推送/同步壳层接缝（L337：NotificationDispatcher 送达口 + PeriodicSyncWorker + WorkManager 装配口径）
    ├── main/java/uniffi/alpha_mobile/alpha_mobile.kt   生成绑定（入库）
    ├── main/jniLibs/arm64-v8a/                          .so（不入库）
    └── test/java/.../  PayloadParsingTest.kt（10 例）+ GestureMappingTest.kt（L367，3 例）
                         JVM 载荷/手势契约单测（无需设备）
```

分层纪律（由 `mobile/tests/android_shell_contract.rs` 11 例守门，CI 无 SDK 也
能防漂移）：跨语言四名一致（cdylib `alpha_mobile` ↔ JNA 载名 ↔ uniffi 命名
空间 ↔ jniLibs 路径）；生成绑定必须暴露壳层在用的 FFI 面；`import uniffi.*`
是 AlphaBridge 的特权，UI 层零生成绑定引用；清单/主题/Gradle 三方一致；
`.gitignore` 分离「生成源入库 / 构建产物不入库」；推送/同步接缝不触碰
uniffi（决策面只在 Rust，docs/mobile-push-sync.md §1）；手势翻译面由
Gestures.kt 独占且映射目标必须真实存在于 AlphaBridge
（docs/mobile-gestures.md §6）。

## 11. 非交互假设（自行判定，已注明）

1. **台账编号**：本文以 L118 记该项（沿用 L112–L117 逐 dispatch 递增的既有顺序，
   TODO.md 行号会随注释增长漂移，不作编号依据）。
2. **UniFFI 走 proc-macro-only**（`setup_scaffolding!`，不写 UDL）：0.25 手册
   明确支持，且消除双份定义漂移；`uniffi = 0.25` / `jni = 0.21` 沿用 workspace
   既有 pin 不升级（升级会牵动未验证的绑定生成器行为）。
3. **JNI 逃生舱本轮不引用**（见 §3 决策）——`jni` pin 保留，首个真实回调场景
   （TODO 270/272）启用。
4. **JSON 字符串桥**而非 `uniffi::Record`（见 §4 理由与演进路径）。
5. **workspace 成员资格**：`mobile` 从 `exclude` 移入 `members`（纯 Rust、无
   平台 SDK 依赖，Linux 门禁可覆盖）；若后续引入 NDK-only 依赖需退回
   target 门控并再评估 exclude。
6. **每次分析新建 current-thread 运行时**（`analyze_symbol` 实为同步计算，执行器
   仅驱动 future）；不引入 `enable_all` 之外的驱动假设，不提前做运行时复用优化。
7. **观察列表外一律 `InvalidSymbol`**（含空 symbol、空列表退化）——能力边界统一
   由核心库判定，平台壳不重复实现。
8. **L301 工具链假设**：派发前提「本环境无 Android SDK」实际已过时——本机实有
   SDK（platforms 34/35/36 + build-tools + NDK r27/28）、Gradle 9.8、JDK 21，故
   Android 侧在本机做了超出 workspace 门禁的实证（绑定生成 + 交叉编译 +
   assembleDebug + JVM 单测）；真机/模拟器运行仍留边界。版本组合取
   Gradle wrapper 8.11.1 + AGP 8.9.0 + Kotlin 1.9.25 + JNA 5.13.0 + Compose BOM
   2024.12.01（本机拉包构建实证，非最新但组合关系明确）；wrapper 由系统
   Gradle 9.8 一次性 bootstrap，门禁与 CI 均不依赖 Android 工具链。
10. **Kotlin 锁 1.9.25**：初版组合取 Kotlin 2.1.0（+compose 插件），K2 编译
    uniffi 0.25 生成绑定报「Overload resolution ambiguity」——错误类的构造
    属性与 override `message` 同名双候选，属 uniffi 0.25 与 Kotlin 2.x 的已知
    不兼容。降级壳侧 Kotlin 1.9.25（composeOptions 1.5.15 官方配对）后实证
    通过；升级 Kotlin 的前置条件是升级 uniffi（workspace pin 0.25 是 L118
    假设②），两层锁必须同进退，契约测试已作负向守卫。
9. **alpha-core android 依赖修复口径**：`jni` 按 target 挂依赖（与 js-sys 的
   wasm 门控同款式），不做 optional feature——TODO 272 启用 JNI 逃生舱时
   android 构建天然带上，语义与 `#[cfg(target_os = "android")]` 的错误映射
   一一对应。

## 12. 真机验收边界（本环境不可观察，登记）

1. 真机加载 `.so` 与运行（L301 已实证到 `assembleDebug` 产物可出、JVM 契约
   单测可跑；crash/符号/装载问题只有目标设备能暴露；x86_64 模拟器镜像需补
   `x86_64` ABI，gen-bindings.sh 加一个 target 即可）。
2. Swift 侧经 staticlib 链接与调用（iOS 无 Xcode 不可执行）。
3. 主线程调用禁忌的实际表现（ANR/jank）与平台壳后台调度约定的落实
   （Kotlin 侧已按 Dispatchers.IO 接线，效果未实测）。
4. 推送 token/生命周期等 SDK 回调经 JNI 逃生舱回灌核心库（TODO 272 的首个
   JNI 实战场）。
5. 多线程并发调用 `MobileCore` 方法（uniffi 对象按 Arc 共享；纯数据 + per-call
   运行时设计上安全，未压测）。

## 13. 后续 TODO 映射

| TODO 项 | 本文依托 | 状态 |
|---|---|---|
| 270 Android Kotlin 环境 | §9 管线 + §10.1 壳 | **L301 已落**（设计文档+骨架+契约单测；真机运行留 §12①） |
| 271 iOS Swift 集成 | §9 staticlib + uniffi Swift 绑定 | 未启（无工具链） |
| 272 推送/后台同步 | §5 错误分类 + `api_url` 配置槽 + JNI 回调 | **L337 已落**（决策面 notify/sync 进核心库 + FFI 六方法只增不改 + Android 接缝；见 docs/mobile-push-sync.md，真机送达/调度留其 §10） |
| 273 触屏手势 | 平台壳职责，不在核心库 | **L367 已落**（手势集三选 + 刷新编排复用 L337 FFI，Rust 零改动；见 docs/mobile-gestures.md，真机手感留其 §8） |
| 274 移动端离线存储与同步 | §7 `api_url` 起点；同步语义参考桌面 L116（kv 快照 + 指纹增量）同口径复用 | 未启 |
