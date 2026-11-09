use super::schema::{TaskTemplate, TaskTemplateFile, TemplateError};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 支持的模板扩展名（architecture 口径 YAML/JSON，不含 config crate 能读的
/// 其余格式——任务定义走声明面，刻意收窄）
const SUPPORTED_EXTS: [&str; 3] = ["yaml", "yml", "json"];

/// 装载模板路径（文件或目录），完成解析 + 装载期校验 + 跨文件 ID 去重。
/// 返回含 disabled 任务在内的全部模板（提交期再过滤）。
pub fn load_path(path: &Path) -> Result<Vec<TaskTemplate>, TemplateError> {
    let meta = std::fs::metadata(path).map_err(|e| TemplateError::Io {
        path: path.display().to_string(),
        source: e,
    })?;

    let files: Vec<PathBuf> = if meta.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|e| TemplateError::Io {
                path: path.display().to_string(),
                source: e,
            })?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| SUPPORTED_EXTS.contains(&e.to_ascii_lowercase().as_str()))
                        .unwrap_or(false)
            })
            .collect();
        // 目录装载按文件名序，提交顺序确定可复现
        entries.sort();
        if entries.is_empty() {
            return Err(TemplateError::Invalid {
                path: path.display().to_string(),
                task: "(目录)".to_string(),
                msg: "目录内没有 *.yaml / *.yml / *.json 模板文件".to_string(),
            });
        }
        entries
    } else {
        vec![path.to_path_buf()]
    };

    let mut templates = Vec::new();
    let mut seen_ids: HashMap<String, String> = HashMap::new();
    let mut duplicates: Vec<String> = Vec::new();
    for file in &files {
        for template in load_file(file)? {
            let origin = file.display().to_string();
            if let Some(first) = seen_ids.get(&template.id) {
                duplicates.push(format!("{}（{} 与 {}）", template.id, first, origin));
            } else {
                seen_ids.insert(template.id.clone(), origin);
            }
            templates.push(template);
        }
    }
    if !duplicates.is_empty() {
        return Err(TemplateError::DuplicateId {
            ids: duplicates.join("、"),
        });
    }
    Ok(templates)
}

/// 解析单个模板文件（扩展名限定 yaml/yml/json）
fn load_file(path: &Path) -> Result<Vec<TaskTemplate>, TemplateError> {
    let display = path.display().to_string();
    let ext_ok = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false);
    if !ext_ok {
        return Err(TemplateError::Parse {
            path: display,
            msg: format!("不支持的扩展名（仅 {}）", SUPPORTED_EXTS.join("/")),
        });
    }

    // config 负责按扩展名选 yaml/json 解析；两跳经 serde_json::Value 走
    // 严格 serde 语义（deny_unknown_fields / 缺字段报错）
    let parsed: TaskTemplateFile = config::Config::builder()
        .add_source(config::File::from(path.to_path_buf()))
        .build()
        .map_err(|e| TemplateError::Parse {
            path: display.clone(),
            msg: e.to_string(),
        })
        .and_then(|cfg| {
            let value: serde_json::Value =
                cfg.try_deserialize().map_err(|e| TemplateError::Parse {
                    path: display.clone(),
                    msg: e.to_string(),
                })?;
            serde_json::from_value(value).map_err(|e| TemplateError::Parse {
                path: display.clone(),
                msg: e.to_string(),
            })
        })?;

    match parsed.version {
        None | Some(1) => {}
        Some(v) => {
            return Err(TemplateError::Invalid {
                path: display,
                task: "(文件)".to_string(),
                msg: format!("不支持的模板版本 {v}（当前仅 1）"),
            })
        }
    }
    if parsed.tasks.is_empty() {
        return Err(TemplateError::Invalid {
            path: display,
            task: "(文件)".to_string(),
            msg: "tasks 列表为空——未定义任何任务".to_string(),
        });
    }

    for (idx, template) in parsed.tasks.iter().enumerate() {
        let label = if template.id.is_empty() {
            format!("第{}项", idx + 1)
        } else {
            template.id.clone()
        };
        template.validate().map_err(|msg| TemplateError::Invalid {
            path: display.clone(),
            task: label,
            msg,
        })?;
    }
    Ok(parsed.tasks)
}
