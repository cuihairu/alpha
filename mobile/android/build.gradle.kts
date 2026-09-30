// L301：混合工程根构建——插件版本集中在此声明，:app 模块只 apply。
// 版本组合（本机 gradle assembleDebug 实证通过，组合依据见 README.md）：
// AGP 8.9.0 需 Gradle 8.11.1+（wrapper 固定 8.11.1）与 JDK 17+。
// Kotlin 锁 1.9.25（compose 编译器走 composeOptions 1.5.15 官方配对）：
// uniffi 0.25 生成的 Kotlin 在 2.x K2 下过载歧义（错误类同名属性），升级
// Kotlin 须先升级 uniffi（另起 TODO，见 docs/mobile-core-architecture.md 假设⑩）。
plugins {
    id("com.android.application") version "8.9.0" apply false
    id("org.jetbrains.kotlin.android") version "1.9.25" apply false
    id("org.jetbrains.kotlin.plugin.serialization") version "1.9.25" apply false
}
