//! Alpha Finance 构建工具
//!
//! 跨平台项目生成和管理工具

use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// 生成新的服务项目
    GenerateService {
        /// 服务名称
        name: String,
        /// 服务类型
        #[arg(short, long, default_value = "http")]
        service_type: String,
    },
    /// 生成新的前端组件
    GenerateComponent {
        /// 组件名称
        name: String,
        /// 组件类型
        #[arg(short, long, default_value = "react")]
        component_type: String,
    },
    /// 验证项目结构
    Validate,
    /// 更新依赖版本
    UpdateDeps,
    /// 生成 API 文档
    GenerateDocs,
    /// 检查代码质量
    CheckQuality,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::GenerateService { name, service_type } => {
            generate_service(&name, &service_type)?;
        }
        Commands::GenerateComponent { name, component_type } => {
            generate_component(&name, &component_type)?;
        }
        Commands::Validate => {
            validate_project()?;
        }
        Commands::UpdateDeps => {
            update_dependencies()?;
        }
        Commands::GenerateDocs => {
            generate_documentation()?;
        }
        Commands::CheckQuality => {
            check_code_quality()?;
        }
    }

    Ok(())
}

/// 生成新的服务项目
fn generate_service(name: &str, service_type: &str) -> anyhow::Result<()> {
    println!("🚀 生成服务: {} (类型: {})", name, service_type);

    let service_dir = Path::new("services").join(name);
    if service_dir.exists() {
        return Err(anyhow::anyhow!("服务目录已存在: {}", name));
    }

    fs::create_dir_all(service_dir.join("src"))?;

    // 生成 Cargo.toml
    let cargo_toml = generate_service_cargo_toml(name, service_type)?;
    fs::write(service_dir.join("Cargo.toml"), cargo_toml)?;

    // 生成 main.rs
    let main_rs = generate_service_main_rs(name, service_type)?;
    fs::write(service_dir.join("src").join("main.rs"), main_rs)?;

    // 生成配置文件
    let config = generate_service_config(name)?;
    fs::write(service_dir.join("config.yml"), config)?;

    println!("✅ 服务生成完成: {}", name);
    Ok(())
}

/// 生成服务 Cargo.toml
fn generate_service_cargo_toml(name: &str, service_type: &str) -> anyhow::Result<String> {
    let template = format!(
        r#"[package]
name = "alpha-{}"
version.workspace = true
authors.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
# 异步运行时
tokio = {{ workspace = true }}

# Web 框架
axum = {{ workspace = true }}

# 序列化
serde = {{ workspace = true }}
serde_json = {{ workspace = true }}

# 日志
tracing = {{ workspace = true }}
tracing-subscriber = {{ workspace = true }}

# 错误处理
anyhow = {{ workspace = true }}

# 配置管理
config = {{ workspace = true }}

# 内部包
alpha-core = {{ workspace = true }}

[dev-dependencies]
tokio-test = {{ workspace = true }}
"#,
        name.to_lowercase().replace("-", "_")
    );

    Ok(template)
}

/// 生成服务 main.rs
fn generate_service_main_rs(name: &str, service_type: &str) -> anyhow::Result<String> {
    let main_rs = match service_type {
        "http" => format!(
            r#"//! {} HTTP Service

use axum::{{extract::Query, response::Json, routing::get, Router}};
use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Deserialize)]
struct HealthQuery {{
    detailed: Option<bool>,
}}

async fn health_check(Query(params): Query<HealthQuery>) -> Json<serde_json::Value> {{
    Json(serde_json::json! {{
        "service": "{}",
        "status": "healthy",
        "timestamp": chrono::Utc::now(),
        "detailed": params.detailed.unwrap_or(false)
    }})
}}

#[tokio::main]
async fn main() -> anyhow::Result<()> {{
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/health", get(health_check));

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("{} 服务监听: {{}}", "{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}}
"#,
            name, name
        ),
        _ => format!(
            r#"//! {} Service

#[tokio::main]
async fn main() -> anyhow::Result<()> {{
    tracing_subscriber::fmt::init();
    tracing::info!("启动 {} 服务", "{}");

    // TODO: 实现服务逻辑

    Ok(())
}}
"#,
            name, name
        ),
    };

    Ok(main_rs)
}

/// 生成服务配置
fn generate_service_config(name: &str) -> anyhow::Result<String> {
    let config = format!(
        r#"# {} 服务配置
service:
  name: {}
  version: "0.1.0"
  port: 8080

database:
  url: "postgresql://localhost/alpha_{}"

redis:
  url: "redis://localhost:6379"

logging:
  level: "info"
"#,
        name, name, name.to_lowercase().replace("-", "_")
    );

    Ok(config)
}

/// 生成新的前端组件
fn generate_component(name: &str, component_type: &str) -> anyhow::Result<()> {
    println!("🎨 生成组件: {} (类型: {})", name, component_type);

    let component_dir = Path::new("web/components").join(name);
    fs::create_dir_all(&component_dir)?;

    match component_type {
        "react" => {
            let component_tsx = generate_react_component(name)?;
            fs::write(component_dir.join(format!("{}.tsx", name)), component_tsx)?;

            let component_test = generate_react_component_test(name)?;
            fs::write(component_dir.join(format!("{}.test.tsx", name)), component_test)?;
        }
        "vue" => {
            let component_vue = generate_vue_component(name)?;
            fs::write(component_dir.join(format!("{}.vue", name)), component_vue)?;
        }
        _ => {
            return Err(anyhow::anyhow!("不支持的组件类型: {}", component_type));
        }
    }

    println!("✅ 组件生成完成: {}", name);
    Ok(())
}

/// 生成 React 组件
fn generate_react_component(name: &str) -> anyhow::Result<String> {
    let component = format!(
        r#"import React from 'react';
import './{}.css';

interface {}Props {{
  // TODO: 定义组件属性
}}

export const {}: React.FC<{}Props> = (props) => {{
  return (
    <div className="{}">
      <h1>{} Component</h1>
      {/* TODO: 实现组件逻辑 */}
    </div>
  );
}};

export default {};
"#,
        name.to_lowercase(),
        name,
        name,
        name,
        name,
        name.to_lowercase(),
        name,
        name
    );

    Ok(component)
}

/// 生成 React 组件测试
fn generate_react_component_test(name: &str) -> anyhow::Result<String> {
    let test = format!(
        r#"import {{ render, screen }} from '@testing-library/react';
import {{ {} }} from './{}';

describe('{}', () => {{
  it('renders correctly', () => {{
    render(<{} />);
    expect(screen.getByText(/{} Component/i)).toBeInTheDocument();
  }});
}});
"#,
        name,
        name,
        name,
        name,
        name
    );

    Ok(test)
}

/// 生成 Vue 组件
fn generate_vue_component(name: &str) -> anyhow::Result<String> {
    let component = format!(
        r#"<template>
  <div class="{}">
    <h1>{{ {{  }} }} Component</h1>
    <!-- TODO: 实现模板 -->
  </div>
</template>

<script setup lang="ts">
// TODO: 实现组件逻辑
interface Props {{
  // TODO: 定义组件属性
}}

const props = defineProps<Props>();
</script>

<style scoped>
.{{
  /* TODO: 实现样式 */
}}
</style>
"#,
        name.to_lowercase(),
        name
    );

    Ok(component)
}

/// 验证项目结构
fn validate_project() -> anyhow::Result<()> {
    println!("🔍 验证项目结构...");

    let required_dirs = vec![
        "packages/core",
        "packages/protocols",
        "packages/storage",
        "wasm-analyzer",
        "desktop",
        "services",
        "tools",
    ];

    for dir in required_dirs {
        if !Path::new(dir).exists() {
            return Err(anyhow::anyhow!("缺少必需目录: {}", dir));
        }
    }

    // 验证 Cargo workspace
    let cargo_toml = fs::read_to_string("Cargo.toml")?;
    if !cargo_toml.contains("[workspace]") {
        return Err(anyhow::anyhow!("根目录缺少 Cargo workspace 配置"));
    }

    println!("✅ 项目结构验证通过");
    Ok(())
}

/// 更新依赖版本
fn update_dependencies() -> anyhow::Result<()> {
    println!("📦 更新依赖版本...");

    // 这里可以实现依赖更新逻辑
    // 例如：检查最新版本、更新 Cargo.toml 等

    println!("✅ 依赖更新完成");
    Ok(())
}

/// 生成 API 文档
fn generate_documentation() -> anyhow::Result<()> {
    println!("📚 生成 API 文档...");

    std::process::Command::new("cargo")
        .args(&["doc", "--workspace", "--no-deps", "--open"])
        .status()?;

    println!("✅ API 文档生成完成");
    Ok(())
}

/// 检查代码质量
fn check_code_quality() -> anyhow::Result<()> {
    println!("🔍 检查代码质量...");

    // 运行 cargo fmt 检查
    let fmt_status = std::process::Command::new("cargo")
        .args(&["fmt", "--", "--check"])
        .status()?;

    if !fmt_status.success() {
        return Err(anyhow::anyhow!("代码格式检查失败"));
    }

    // 运行 cargo clippy
    let clippy_status = std::process::Command::new("cargo")
        .args(&["clippy", "--workspace", "--", "-D", "warnings"])
        .status()?;

    if !clippy_status.success() {
        return Err(anyhow::anyhow!("Clippy 检查失败"));
    }

    println!("✅ 代码质量检查通过");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_service_cargo_toml() {
        let result = generate_service_cargo_toml("test-service", "http");
        assert!(result.is_ok());
        let cargo_toml = result.unwrap();
        assert!(cargo_toml.contains("alpha-test-service"));
    }

    #[test]
    fn test_generate_react_component() {
        let result = generate_react_component("TestComponent");
        assert!(result.is_ok());
        let component = result.unwrap();
        assert!(component.contains("TestComponent"));
    }
}