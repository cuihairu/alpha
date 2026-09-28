fn main() -> Result<(), Box<dyn std::error::Error>> {
    // grpc feature 关闭（如 wasm32 --no-default-features 取纯 serde 契约层）时，
    // tonic-build 作为 optional build-dependency 不参与编译，这里 cfg 门控跳过代码生成
    #[cfg(feature = "grpc")]
    {
        tonic_build::configure()
            .build_client(true)
            .build_server(true)
            .compile(&["proto/data_engine.proto"], &["proto"])?;
    }
    // 非 grpc 构建：无 proto 产物，lib.rs 的 proto 模块亦被 cfg 排除
    #[cfg(not(feature = "grpc"))]
    {}
    Ok(())
}
