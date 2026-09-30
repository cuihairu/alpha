// L301：Android Kotlin + Rust 混合工程（Jetpack Compose 壳 + alpha-mobile
// UniFFI 绑定）。仓库布局与生成流程见同目录 gen-bindings.sh、README.md 与
// docs/mobile-core-architecture.md §10。
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositoriesMode = RepositoriesMode.FAIL_ON_PROJECT_REPOS
    repositories {
        google()
        mavenCentral()
    }
}
rootProject.name = "alpha-mobile-android"
include(":app")
