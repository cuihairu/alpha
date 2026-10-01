# Android APK/AAB 分包与多渠道发布（TODO L517）

落地三件：**双 ABI 原生库**（`gen-bindings.sh` 循环化，分包的真实前提）、
**gradle 渠道/分包矩阵**（`app/build.gradle.kts`）、**一键构建编排**
（`scripts/android-release.sh`）。本机已实测出全渠道产物。

## 1. 渠道矩阵

| 渠道 | flavor | 产物 | 分发方式 | 分包语义 |
|---|---|---|---|---|
| Google Play | `play` | `app-play-release.aab` | Play Console 上架 | Play 按设备 ABI/密度/语言**动态分包**下发（AAB 原生能力，无需配置） |
| Google Play（备用） | `play` | `app-{arm64-v8a,x86_64}-release.apk` | 不上架，构建副产物 | `splits.abi` 按 ABI 拆分 |
| 官网/侧载 | `direct` | `app-universal-release.apk` | 官网直链/应用市场代收 | `universalApk = true` 单文件含双 ABI |

- `applicationId` 各渠道一致（`com.alpha.finance.mobile`）——同一应用
  身份，不搞渠道包多包名；渠道以 `BuildConfig.DISTRIBUTION_CHANNEL`
  标记（`buildConfig = true`），供运行期统计与更新检查区分。
- `versionCode` 各渠道同一值：Play 只收 AAB 无撞号问题；官网直发
  单文件多 ABI 包，升级判定用 `versionName`。

## 2. 双 ABI（分包的前提）

`gen-bindings.sh` 由单 arm64 循环化为 **arm64-v8a（真机）+ x86_64
（模拟器/Chromebook）**：逐 ABI `rustup target add` + NDK API-26 clang
链接器（缺链接器显式报错）→ `jniLibs/<abi>/libalpha_mobile.so`。
本机实测：双 .so 32MB 级（debug 未 strip；release 出包走 gradle 打包
时仅打包不重编，体积随 release profile 下降）。

新增 ABI 步骤：循环里加一行 `<rust-triple>:<jni-abi>` + 确认 NDK 有对应
`${triple}${api}-clang` 包装器。

## 3. 签名

四个环境变量（`ALPHA_KEYSTORE_PATH` / `ALPHA_KEYSTORE_PASSWORD` /
`ALPHA_KEY_ALIAS` / `ALPHA_KEY_PASSWORD`）齐 → release 签名出包；
缺省 unsigned（可构建不可安装，CI 冒烟语义）。**keystore 与密码绝不
入库**——CI 注入 secret（接线归 L470），本机放 `~/.android/` 之外。

Play 上架 2026 起 AAB 强制 Play App Signing：上传密钥（上表）只签
上传件，正式分发签名由 Play 托管——密钥丢失可用 Play 重置上传密钥，
注册进 L471 商店集成。

## 4. 边界登记

- **R8/混淆**：仍关闭——uniffi 生成面靠 JNA 反射，keep 规则
  （`net.java.dev.jna.**`、uniffi 回调面）归 L470 发布流水线做；
- **versionCode 多 ABI 区分**：官网直发若未来按 ABI 分发（而非
  universal），需 `versionCodeOverride` 按 ABI 偏移——用户量不支持前
  不做；
- **国内应用市场代收**（华为/小米等）：direct universal APK 通用，
  各市场后台配置归 L471。

## 5. 非交互假设

1. 双渠道足够（Play + 官网直发）；更多渠道=加 flavor 一行，不预建；
2. universal APK 作为官网直发唯一形态（当前双 ABI 总增量 ~16MB debug，
   release 后可接受）；用户反馈存储敏感时再启按 ABI 直发；
3. `.so` 入 `jniLibs` 不入库（.gitignore 既有约定），CI 构建前先跑
   gen-bindings.sh（或缓存产物）。
