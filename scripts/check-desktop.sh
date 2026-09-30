#!/usr/bin/env bash
# 桌面端框架门禁（TODO L112「搭建 Tauri + Rust 桌面应用框架」的构建门禁；非交互）
#
# 分层依据（Tauri 1.x 在 Linux 需 WebKitGTK + libsoup 系统库，CI 的 Linux runner 没有）：
#   * 框架层（desktop 的 lib，零 Tauri 依赖）—— 在任意 Linux runner 可编译/测试：
#     clippy -D warnings + 单测；
#   * GUI 接线层（gui 特性：命令 + generate_context!）—— 需 GUI 依赖，由 CI 的
#     Desktop (macOS) 作业 `cargo test -p alpha-desktop --all-targets` 验证。
#
# 本脚本的可移植部分（[1/4]）是**配置自洽性门禁**，抓的是「配置与代码漂移」类红灯：
#   1. tauri.conf.json 可解析且必填字段齐全（identifier/distDir/devPath/窗口尺寸）；
#   2. bundle.icon 列出的文件真实存在（否则打包期才炸）；
#   3. distDir 存在且含 index.html（否则窗口全白——骨架的核心诉求之一）；
#   4. 兜底壳用的 Tauri 版本契约：全局 API 开关在 **build** 段（v1 语义；v2 才是
#      tauri 段，字段名/段位置写错只会在 tauri_build::build() 运行时才炸），
#      且兜底壳代码里不得出现 v2 的 ipcRenderer（v1 全局 API 直接提供 invoke）；
#   5. allowlist 放开的 API 在 desktop/Cargo.toml 里确实启用了对应 tauri 特性
#      （allowlist 与 Rust feature 不一致是运行期 panic 的经典来源）；
#   6. 无孤儿配置：真正的 crate 根是 desktop/，desktop/src-tauri/ 不应存在
#      （早期脚手架残留的 tauri.conf.json 会让人改错文件）。
#
# 注：配置 schema 本身（字段名/层级是否匹配 Tauri 1.x 的 deny_unknown_fields）
# 由 tests/tauri_config.rs 用 tauri-utils 走 tauri-build 同一条解析路径校验，
# 覆盖在 [4/4]；本脚本只做无需编译的快速检查。
#
# 用法：scripts/check-desktop.sh

set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "❌ $*"; exit 1; }
info() { echo "ℹ️  $*"; }
ok()   { echo "✅ $*"; }

echo "=== 桌面端框架门禁 ==="

echo "--- [1/4] tauri.conf.json 配置自洽性"
python3 - <<'PY' || exit 1
import json, pathlib, re, sys

root = pathlib.Path(".").resolve()
crate = root / "desktop"
conf_path = crate / "tauri.conf.json"
errors = []

try:
    conf = json.loads(conf_path.read_text(encoding="utf-8"))
except Exception as exc:  # noqa: BLE001
    print(f"❌ tauri.conf.json 解析失败: {exc}")
    sys.exit(1)

tauri = conf.get("tauri", {})
build = conf.get("build", {})
pkg = conf.get("package", {})

# 1) 必填字段
if not tauri.get("bundle", {}).get("identifier"):
    errors.append("tauri.bundle.identifier 缺失")
if not pkg.get("productName"):
    errors.append("package.productName 缺失")
if not pkg.get("version"):
    errors.append("package.version 缺失")
dev_path = build.get("devPath", "")
if not dev_path.startswith(("http://", "https://", "file://")):
    errors.append(f"build.devPath 不是可加载的 URL: {dev_path!r}")

windows = tauri.get("windows") or []
if not windows:
    errors.append("tauri.windows 为空：应用没有窗口")
for i, win in enumerate(windows):
    for key in ("width", "height", "minWidth", "minHeight"):
        if not isinstance(win.get(key), (int, float)) or win.get(key) <= 0:
            errors.append(f"tauri.windows[{i}].{key} 非法: {win.get(key)!r}")

# 2) 图标真实存在
icons = tauri.get("bundle", {}).get("icon") or []
if not icons:
    errors.append("tauri.bundle.icon 为空")
for icon in icons:
    if not (crate / icon).is_file():
        errors.append(f"图标缺失: desktop/{icon}")

# 3) distDir 必须有内容（否则 Tauri 窗口全白）
dist = build.get("distDir", "")
if not dist:
    errors.append("build.distDir 缺失")
else:
    dist_dir = (crate / dist).resolve()
    if not dist_dir.is_dir():
        errors.append(f"build.distDir 目录不存在: {dist_dir}")
    elif not (dist_dir / "index.html").is_file():
        errors.append(
            f"build.distDir 缺少 index.html（窗口会全白）: {dist_dir}/index.html"
        )

# 4) 兜底壳依赖全局 API → build 段必须 withGlobalTauri（v1 语义）
if not build.get("withGlobalTauri"):
    errors.append(
        "build.withGlobalTauri 未开启：distDir 兜底壳通过 window.__TAURI__ 调 Rust 命令"
    )
if tauri.get("withGlobalTauri") is not None:
    errors.append(
        "tauri.withGlobalTauri 不是 Tauri 1.x 的合法字段（v1 该字段属于 build 段，"
        "放在 tauri 段会让 tauri_build::build() 因 deny_unknown_fields 直接失败）"
    )

# 4b) 兜底壳代码的 IPC 入口必须是 v1 的 window.__TAURI__.invoke
if dist_dir.is_dir():
    shell_path = dist_dir / "desktop-shell.js"
    if not shell_path.is_file():
        errors.append(f"兜底壳脚本缺失: {shell_path}")
    else:
        shell = shell_path.read_text(encoding="utf-8")
        # 去掉注释后再断言：注释里可以解释「为什么不用 v2 的 ipcRenderer」
        code = re.sub(r"/\*.*?\*/", "", shell, flags=re.S)
        code = re.sub(r"(?m)^\s*//.*$", "", code)
        if "window.__TAURI__" not in code:
            errors.append("兜底壳未使用 window.__TAURI__（需 build.withGlobalTauri 注入）")
        if "ipcRenderer" in code:
            errors.append(
                "兜底壳代码出现 ipcRenderer：那是 Tauri v2 的 API，v1 全局 API 直接提供 invoke"
            )

# 5) allowlist 与 Cargo.toml 的 tauri 特性对齐
allow = tauri.get("allowlist", {})
expected = set()
if (allow.get("shell") or {}).get("open"):
    expected.add("shell-open")
if (allow.get("dialog") or {}).get("open"):
    expected.add("dialog-open")
if (allow.get("dialog") or {}).get("save"):
    expected.add("dialog-save")
fs = allow.get("fs") or {}
for conf_key, feature in {
    "readFile": "fs-read-file",
    "writeFile": "fs-write-file",
    "readDir": "fs-read-dir",
    "copyFile": "fs-copy-file",
    "createDir": "fs-create-dir",
    "removeFile": "fs-remove-file",
    "removeDir": "fs-remove-dir",
    "renameFile": "fs-rename-file",
    "exists": "fs-exists",
}.items():
    if fs.get(conf_key):
        expected.add(feature)
if (allow.get("path") or {}).get("all"):
    expected.add("path-all")
if (allow.get("notification") or {}).get("all"):
    expected.add("notification-all")
if (allow.get("globalShortcut") or {}).get("all"):
    expected.add("global-shortcut-all")
if tauri.get("systemTray"):
    expected.add("system-tray")

manifest = (crate / "Cargo.toml").read_text(encoding="utf-8")
m = re.search(r"^tauri\s*=\s*\{(.*?)\}", manifest, re.M | re.S)
if not m:
    errors.append("desktop/Cargo.toml 未声明 tauri 依赖（应作为 gui 特性下的可选依赖）")
else:
    features = set(re.findall(r'"([a-z0-9-]+)"', m.group(1).split("features")[1])) if "features" in m.group(1) else set()
    for feature in sorted(expected):
        if feature not in features:
            errors.append(
                f"allowlist 放开了 {feature}，但 desktop/Cargo.toml 的 tauri 特性未启用"
            )

# 6) 孤儿配置：crate 根是 desktop/，src-tauri 不应存在
orphan = crate / "src-tauri"
if orphan.exists():
    errors.append(
        f"孤儿目录 desktop/src-tauri 存在（crate 根是 desktop/）：{orphan} —— 其中若有 tauri.conf.json 会被误改"
    )

if errors:
    for err in errors:
        print(f"❌ {err}")
    sys.exit(1)
print(f"✅ tauri.conf.json 自洽（图标 {len(icons)} 个 / distDir {dist}）")
PY
ok "配置自洽性检查通过"

echo "--- [2/4] 无孤儿 Tauri 配置"
[ ! -e desktop/src-tauri ] || fail "desktop/src-tauri 应删除（crate 根是 desktop/）"
ok "无孤儿配置"

echo "--- [3/4] 框架层 clippy（--no-default-features，无需 GUI 系统库）"
cargo clippy -p alpha-desktop --no-default-features --all-targets -- -D warnings
ok "框架层 clippy 零警告"

echo "--- [4/4] 框架层单测 + tauri.conf.json 契约测试（--no-default-features）"
cargo test -p alpha-desktop --no-default-features --all-targets
ok "框架层单测通过（含 tests/tauri_config.rs 配置契约）"

echo "=== 桌面端框架门禁全部通过 ==="
info "GUI 接线层（gui 特性）由 CI Desktop (macOS) 作业编译验证：cargo test -p alpha-desktop --all-targets"