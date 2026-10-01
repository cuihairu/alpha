//! Rust 内存安全监控（L463）：unsafe 清单扫描 + 舱位预算断言。
//!
//! 与 [`crate::alloc_tracking`]（运行期分配/泄漏计数）互补——本模块是
//! **静态面**的：把「哪里写了 unsafe、每处属于哪类风险」变成可枚举、可断言的
//! 数据，新增 unsafe 必须同步抬预算（conscious ack），删掉 unsafe 顺手回收。
//!
//! 舱位隔离（双层防线，本单在 alpha-core 落地）：
//! - **编译期**：`lib.rs` 顶部 `#![deny(unsafe_code)]` 硬拒；仅 [`crate::simd`]
//!   （target_feature intrinsics）与 [`crate::alloc_tracking`]（GlobalAlloc 契约）
//!   两个舱位模块顶部 `#![allow(unsafe_code)]` 豁免——新写 unsafe 编译即败；
//! - **审计期**（本模块）：全仓扫描断言舱位白名单 + 类别预算——白名单外出现
//!   unsafe、或白名单内总量超预算，测试即红（deny 只护 alpha-core，扫描口径
//!   覆盖 packages/services/tools 全仓，其余 crate 编译期门控登记为后续硬化项）。
//!
//! 扫描口径刻意保守：只认**行首 token 级别**的关键字与已知危险 API，
//! 注释/文档注释行剔除，多行块注释按行剔除首尾；宁可漏报也不误报
//! （误报会让预算失去信号价值）。

use std::collections::BTreeMap;
use std::fmt;

/// unsafe 风险类别（分类互斥，按 `classify` 优先级判定）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnsafeKind {
    /// `unsafe fn` 定义：函数体内被整体豁免调用检查
    UnsafeFn,
    /// `unsafe impl`：类型/trait 契约由手写保证（如 `GlobalAlloc`）
    UnsafeImpl,
    /// `unsafe { ... }` 块
    UnsafeBlock,
    /// `std::mem::transmute` / `transmute_unchecked`
    Transmute,
    /// 裸指针构造或解引用（`*const` / `*mut` 类型标注、`ptr::read/write`）
    RawPointer,
    /// `slice::from_raw_parts*` / `str::from_utf8_unchecked`
    FromRawParts,
}

impl UnsafeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            UnsafeKind::UnsafeFn => "unsafe_fn",
            UnsafeKind::UnsafeImpl => "unsafe_impl",
            UnsafeKind::UnsafeBlock => "unsafe_block",
            UnsafeKind::Transmute => "transmute",
            UnsafeKind::RawPointer => "raw_pointer",
            UnsafeKind::FromRawParts => "from_raw_parts",
        }
    }
}

impl fmt::Display for UnsafeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 一处 unsafe 发现：`file` 为仓内相对路径，`line` 为 1 基行号
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub kind: UnsafeKind,
}

/// 全仓清单摘要
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    pub findings: Vec<Finding>,
}

impl Inventory {
    pub fn total(&self) -> usize {
        self.findings.len()
    }

    /// 按文件分组计数（保持文件名字典序，便于 diff 审阅）
    pub fn by_file(&self) -> BTreeMap<&str, usize> {
        let mut m: BTreeMap<&str, usize> = BTreeMap::new();
        for f in &self.findings {
            *m.entry(f.file.as_str()).or_insert(0) += 1;
        }
        m
    }

    /// 按类别分组计数（见 [`Inventory::by_kind_of`]）
    pub fn by_kind(&self) -> BTreeMap<&'static str, usize> {
        Self::by_kind_of(&self.findings)
    }

    /// 按类别分组计数（全类别预注册零值——面板展示不用处理缺省）。
    /// 关联函数形态供「已过滤的 findings 子集」（如只统计真 unsafe 舱位）复用。
    pub fn by_kind_of(findings: &[Finding]) -> BTreeMap<&'static str, usize> {
        let mut m: BTreeMap<&'static str, usize> = BTreeMap::new();
        for kind in [
            UnsafeKind::UnsafeFn,
            UnsafeKind::UnsafeImpl,
            UnsafeKind::UnsafeBlock,
            UnsafeKind::Transmute,
            UnsafeKind::RawPointer,
            UnsafeKind::FromRawParts,
        ] {
            m.entry(kind.as_str()).or_insert(0);
        }
        for f in findings {
            *m.entry(f.kind.as_str()).or_insert(0) += 1;
        }
        m
    }

    /// 命中舱位的文件清单（文件名 → 该文件 unsafe 处数）
    pub fn files_with_findings(&self) -> Vec<&str> {
        self.by_file().into_keys().collect()
    }
}

/// 类别数预算：某类别新增即视为 conscious ack 的抬预算动作
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindBudget {
    pub kind: UnsafeKind,
    pub max: usize,
}

/// 预算断言结果：违规项为空表示通过
pub fn audit(findings: &[Finding], budgets: &[KindBudget]) -> Vec<String> {
    let mut by_kind: BTreeMap<&'static str, usize> = BTreeMap::new();
    for f in findings {
        *by_kind.entry(f.kind.as_str()).or_insert(0) += 1;
    }
    let mut violations = Vec::new();
    for b in budgets {
        let actual = by_kind.get(b.kind.as_str()).copied().unwrap_or(0);
        if actual > b.max {
            violations.push(format!(
                "{}: {} 处 > 预算 {} 处（新增 unsafe 需显式抬预算并复核不变量）",
                b.kind, actual, b.max
            ));
        }
    }
    violations
}

/// 剔除注释后的代码残渣：整行注释、文档注释、块注释首尾行移除。
/// 行内 `//` 之后的说明文字同样剔除（`'//'` 与 `'"'` 处理见下）。
fn strip_comments(line: &str) -> &str {
    // 行内注释起点：不在字符串字面量内的 `//`（本仓源码无含 `//` 的字面量，
    // 保守处理即可——漏判只会多留残渣，不会引入误报）
    let cut = line.find("//").unwrap_or(line.len());
    line[..cut].trim()
}

/// 单行分类：返回该行命中的**唯一最高优先级**类别（互斥）。
pub fn classify(line: &str) -> Option<UnsafeKind> {
    let code = strip_comments(line);
    if code.is_empty() {
        return None;
    }
    // 优先级：块内调用 > 契约类定义 > 转换类 API
    // （`unsafe impl` 行里若同时含 `unsafe fn` 方法签名，取 impl——行级归并足够）
    if code.contains("unsafe {") || code.contains("unsafe{") {
        return Some(UnsafeKind::UnsafeBlock);
    }
    if code.contains("unsafe impl") {
        return Some(UnsafeKind::UnsafeImpl);
    }
    if code.contains("unsafe fn") || code.contains("unsafe extern") {
        return Some(UnsafeKind::UnsafeFn);
    }
    if code.contains("transmute") {
        return Some(UnsafeKind::Transmute);
    }
    if code.contains("from_raw_parts") || code.contains("from_utf8_unchecked") {
        return Some(UnsafeKind::FromRawParts);
    }
    if code.contains("*const ")
        || code.contains("*mut ")
        || code.contains("ptr::read")
        || code.contains("ptr::write")
    {
        return Some(UnsafeKind::RawPointer);
    }
    None
}

/// 扫描单个源文件：`file` 为仓内相对路径（写入 findings 供定位）
pub fn scan_file(file: &str, src: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut in_block_comment = false;
    for (idx, raw) in src.lines().enumerate() {
        let trimmed = raw.trim();
        if in_block_comment {
            if trimmed.contains("*/") {
                in_block_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_block_comment = true;
            continue;
        }
        if let Some(kind) = classify(raw) {
            out.push(Finding {
                file: file.to_string(),
                line: idx + 1,
                kind,
            });
        }
    }
    out
}

/// 扫描多文件（`(相对路径, 源码)` 列表）
pub fn scan_all(files: &[(&str, &str)]) -> Inventory {
    let mut findings = Vec::new();
    for (file, src) in files {
        findings.extend(scan_file(file, src));
    }
    Inventory { findings }
}

/// 舱位违规项：`allow_unsafe_files` 为声明 `#[allow(unsafe_code)]` 的文件白名单，
/// 白名单外的任何 unsafe 发现即越舱。
pub fn quarantine_violations(findings: &[Finding], allow_unsafe_files: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = findings
        .iter()
        .filter(|f| !allow_unsafe_files.iter().any(|a| f.file == *a))
        .map(|f| {
            format!(
                "{}:{}: {} 出现在非舱位文件（须移除或移入 #[allow(unsafe_code)] 白名单）",
                f.file, f.line, f.kind
            )
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
//! 模块文档：提一句 unsafe impl 不应被计入
/// doc: unsafe fn f()
use std::mem;

/* 块注释里的
   unsafe fn hidden() {} 也不计数 */
pub fn demo(v: &[f64]) -> f64 {
    // 行内注释：unsafe { panic!() }
    let p: *const f64 = v.as_ptr(); // 裸指针
    let n = unsafe { sum_raw(p, v.len()) };
    let _ = unsafe { std::mem::transmute::<u8, i8>(1) };
    n
}

unsafe fn sum_raw(_p: *const f64, _n: usize) -> f64 {
    let s = unsafe { std::slice::from_raw_parts(_p, _n) };
    s.iter().sum()
}
"#;

    #[test]
    fn classify_covers_all_kinds() {
        assert_eq!(classify("unsafe fn f() {}"), Some(UnsafeKind::UnsafeFn));
        assert_eq!(
            classify("unsafe impl Sync for X {}"),
            Some(UnsafeKind::UnsafeImpl)
        );
        assert_eq!(
            classify("    let x = unsafe { f() };"),
            Some(UnsafeKind::UnsafeBlock)
        );
        assert_eq!(
            classify("mem::transmute::<u8, i8>(0)"),
            Some(UnsafeKind::Transmute)
        );
        assert_eq!(
            classify("let p: *mut u8 = q;"),
            Some(UnsafeKind::RawPointer)
        );
        assert_eq!(
            classify("std::slice::from_raw_parts(p, n)"),
            Some(UnsafeKind::FromRawParts)
        );
        assert_eq!(classify("let s = String::new();"), None);
    }

    #[test]
    fn comments_never_count_as_unsafe() {
        assert_eq!(classify("// unsafe impl Send for X {}"), None);
        assert_eq!(classify("/// doc unsafe fn g()"), None);
        assert_eq!(classify("//!"), None);
        assert_eq!(classify(""), None);
    }

    #[test]
    fn scan_file_skips_doc_block_and_inline_comments() {
        let findings = scan_file("pkg/demo.rs", SRC);
        // 期望命中：*const 裸指针、3 个 unsafe block、unsafe fn
        // （`unsafe { transmute/from_raw_parts }` 行按 classify 优先级归并为 block——
        //   行级归并足够，转换类 API 单独成类只认裸调用行）
        let kinds: Vec<UnsafeKind> = findings.iter().map(|f| f.kind).collect();
        assert_eq!(
            kinds,
            vec![
                UnsafeKind::RawPointer,  // let p: *const f64
                UnsafeKind::UnsafeBlock, // unsafe { sum_raw }
                UnsafeKind::UnsafeBlock, // unsafe { transmute }（块优先）
                UnsafeKind::UnsafeFn,    // unsafe fn sum_raw
                UnsafeKind::UnsafeBlock, // unsafe { from_raw_parts }（块优先）
            ]
        );
        // 行号从 1 起：裸指针在第 10 行
        assert_eq!(findings[0].line, 10);
        assert!(findings.iter().all(|f| f.file == "pkg/demo.rs"));
    }

    #[test]
    fn inventory_aggregates_by_file_and_kind() {
        let inv = scan_all(&[
            ("a.rs", "unsafe fn x() {}"),
            ("a.rs", "unsafe { y(); }"),
            ("b.rs", "unsafe impl Z {}"),
        ]);
        assert_eq!(inv.total(), 3);
        assert_eq!(inv.by_file().get("a.rs"), Some(&2));
        assert_eq!(inv.by_kind().get("unsafe_fn"), Some(&1));
        assert_eq!(inv.by_kind().get("unsafe_block"), Some(&1));
        // 未出现的类别仍列 0——面板展示不用处理缺省
        assert_eq!(inv.by_kind().get("transmute"), Some(&0));
        assert_eq!(inv.files_with_findings(), vec!["a.rs", "b.rs"]);
    }

    #[test]
    fn budget_blocks_new_unsafe_kinds() {
        let inv = scan_all(&[("a.rs", "unsafe fn x() {}\nunsafe impl Z {}")]);
        let v = audit(
            &inv.findings,
            &[
                KindBudget {
                    kind: UnsafeKind::UnsafeFn,
                    max: 1,
                },
                KindBudget {
                    kind: UnsafeKind::UnsafeImpl,
                    max: 1,
                },
            ],
        );
        assert!(v.is_empty(), "预算内不应违规: {v:?}");

        let v2 = audit(
            &inv.findings,
            &[KindBudget {
                kind: UnsafeKind::UnsafeFn,
                max: 0,
            }],
        );
        assert_eq!(v2.len(), 1);
        assert!(v2[0].contains("unsafe_fn"));
        assert!(v2[0].contains("1 处 > 预算 0 处"));
    }

    #[test]
    fn quarantine_flags_unsafe_outside_declared_modules() {
        let inv = scan_all(&[
            ("packages/core/src/simd.rs", "unsafe fn sum() {}"),
            ("packages/core/src/models.rs", "unsafe { leak() }"),
        ]);
        let v = quarantine_violations(&inv.findings, &["packages/core/src/simd.rs"]);
        assert_eq!(v.len(), 1);
        assert!(v[0].contains("models.rs:1"));
        assert!(v[0].contains("非舱位"));
    }

    /// 递归收集 `.rs`（跳过构建/依赖产物目录）
    fn collect_rs(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if matches!(name.as_ref(), "target" | "node_modules" | ".git" | "dist") {
                    continue;
                }
                collect_rs(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                if let Ok(src) = std::fs::read_to_string(&path) {
                    // 相对 workspace 根、`/` 分隔（与舱位白名单口径一致）
                    let rel = path
                        .strip_prefix(
                            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
                        )
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.push((rel, src));
                }
            }
        }
    }

    /// 全仓扫描（L463 门禁面）：packages/services/tools 全部 `.rs` 过扫描——
    /// 舱位白名单外零 unsafe（越舱即红），白名单内按类别预算锁基线。
    /// 报告入口：`cargo test -p alpha-core workspace_stays -- --nocapture`。
    #[test]
    fn workspace_stays_in_quarantine_with_kind_budget() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files: Vec<(String, String)> = Vec::new();
        for dir in ["packages", "services", "tools"] {
            collect_rs(&root.join(dir), &mut files);
        }
        assert!(!files.is_empty(), "扫描文件数为 0——路径口径失效");
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, s)| (p.as_str(), s.as_str()))
            .collect();
        let inv = scan_all(&refs);

        // 舱位：simd（intrinsics）、alloc_tracking（GlobalAlloc）两处真 unsafe 豁免；
        // safety_audit.rs 自身豁免——审计工具的模式匹配表与测试样本必然携带
        // unsafe 关键词（均为字符串字面量；lib.rs deny 对真 unsafe 编译期兜底）
        let allow = [
            "packages/core/src/simd.rs",
            "packages/core/src/alloc_tracking.rs",
            "packages/core/src/safety_audit.rs",
        ];
        let violations = quarantine_violations(&inv.findings, &allow);
        assert!(
            violations.is_empty(),
            "越舱 unsafe（新出现须移除或显式扩舱位并复核）：\n{}",
            violations.join("\n")
        );

        // 类别预算 = 真 unsafe 舱位实测基线（safety_audit.rs 只在上方白名单里
        // 参与豁免，其模式表/测试样本是字符串字面量，不进预算——否则工具自身
        // 演化（改一个 classify 模式）会撞预算产生噪声信号）
        let real: Vec<Finding> = inv
            .findings
            .iter()
            .filter(|f| f.file != "packages/core/src/safety_audit.rs")
            .cloned()
            .collect();
        let by_kind = Inventory::by_kind_of(&real);
        let budgets = [
            (UnsafeKind::UnsafeBlock, 12),
            (UnsafeKind::UnsafeFn, 4),
            (UnsafeKind::UnsafeImpl, 1),
            (UnsafeKind::Transmute, 0),
            (UnsafeKind::RawPointer, 0),
            (UnsafeKind::FromRawParts, 0),
        ];
        let budgets: Vec<KindBudget> = budgets
            .into_iter()
            .map(|(kind, max)| KindBudget { kind, max })
            .collect();
        let over = audit(&real, &budgets);
        assert!(
            over.is_empty(),
            "unsafe 超预算（conscious ack：新增需抬预算并复核不变量）：\n{}\n实测分布: {by_kind:?}\n清单: {real:?}",
            over.join("\n")
        );
    }
}
