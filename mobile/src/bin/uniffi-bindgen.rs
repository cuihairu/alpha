//! 绑定生成 CLI（L301）：`cargo run -p alpha-mobile --features bindgen --bin
//! uniffi-bindgen -- generate --library <libalpha_mobile.so> --language kotlin
//! --out-dir <app/src/main/java>`。scaffolding 已由 `setup_scaffolding!` 在
//! 编译期进库，CLI 只从编译产物读元数据生成 Kotlin/Swift 绑定——用法见
//! mobile/android/gen-bindings.sh 与 docs/mobile-core-architecture.md §10。

fn main() {
    uniffi::uniffi_bindgen_main()
}
