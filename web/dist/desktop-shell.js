/**
 * 桌面骨架兜底壳的前端逻辑（TODO L112）。
 *
 * 只用 Tauri v1 全局 API（window.__TAURI__，由 tauri.conf.json 的 build.withGlobalTauri
 * 注入；v1 全局 API 是 window.__TAURI__.invoke(cmd, args)，没有 v2 的 ipcRenderer），
 * 不引入 npm 依赖 —— 保证 distDir 只有这两个文件时窗口依然可用。命令对应
 * desktop/src/gui.rs 的 #[tauri::command]（Tauri 1.x 参数键默认 camelCase，
 * 多词参数必须写 targetPrice/alertType/filePath，写 snake 会在运行期静默失配）：
 *   initialize_app → get_app_info → get_real_time_quotes → analyze_symbol
 * 链路终点是 alpha-core 的 AnalysisEngine（真实计算，非桩数据）；另有导出卡片经
 * 原生另存为对话框调 export_symbol_to_file（TODO L113），保存位置由用户自选；
 * 告警卡片（TODO L114）走 set_price_alert → check_alerts → set_tray_status：
 * 触发告警由 Rust 侧弹系统通知并入队，托盘 tooltip 反映告警状态。
 * 主题（TODO L115）：配置 theme=light/dark 强制覆盖，system（或缺省）跟随
 * prefers-color-scheme 并实时响应系统切换——Tauri 1.x 原生装饰无运行期
 * set_theme（窗口主题由 tauri.conf.json 创建期跟随），覆盖落在内容层
 * data-theme + CSS 变量（index.html）。窗口几何持久化在 Rust 侧，不经前端。
 *
 * 非 Tauri 环境（普通浏览器直接打开 web/dist/index.html）不报错，改为提示先构建前端。
 */

(function () {
  "use strict";

  var api = window.__TAURI__ || null;
  var invoke = api && typeof api.invoke === "function" ? api.invoke.bind(api) : null;
  var banner = document.getElementById("banner");

  function setText(id, text, cls) {
    var el = document.getElementById(id);
    el.textContent = text;
    if (cls) el.className = cls;
  }

  function kv(label, value) {
    return '<div class="kv"><span>' + label + "</span><span>" + value + "</span></div>";
  }

  function esc(value) {
    return String(value === null || value === undefined ? "" : value).replace(/[&<>"]/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c];
    });
  }

  function num(value, digits) {
    return value === null || value === undefined ? "—" : Number(value).toFixed(digits);
  }

  if (!invoke) {
    banner.textContent = "未检测到 Tauri 运行时";
    document.getElementById("hint").innerHTML =
      "本页面是桌面端兜底壳：请用 <code>cargo run -p alpha-desktop</code> 或 <code>tauri dev</code> 启动窗口；" +
      "若要嵌入完整 Web 前端（含 WASM 分析引擎），先执行 <code>cd web && npm run build</code>，" +
      "构建产物会覆盖本目录中的 <code>index.html</code>。";
    return;
  }

  document.getElementById("hint").innerHTML =
    "此窗口由 Rust 侧 Tauri 应用托管；下方数据来自 alpha-core 计算引擎（Rust）。" +
    "完整 Web 前端构建后（<code>cd web && npm run build</code>）会替换本壳。";

  var symbols = [];
  var firstQuote = null;

  // L115 主题适配：system（或缺省/未知）跟随系统且实时切换；light/dark 强制覆盖。
  // matchMedia 兼容旧 WebKit 的 addListener 回退；无 matchMedia 时按浅色渲染。
  var themeQuery = window.matchMedia ? window.matchMedia("(prefers-color-scheme: dark)") : null;
  var currentTheme = "system";

  function applyTheme(pref) {
    currentTheme = pref || "system";
    var dark =
      currentTheme === "dark" ||
      (currentTheme === "system" && themeQuery && themeQuery.matches);
    document.documentElement.setAttribute("data-theme", dark ? "dark" : "light");
  }

  function onSystemThemeChange() {
    applyTheme(currentTheme);
  }

  if (themeQuery) {
    if (typeof themeQuery.addEventListener === "function") {
      themeQuery.addEventListener("change", onSystemThemeChange);
    } else if (typeof themeQuery.addListener === "function") {
      themeQuery.addListener(onSystemThemeChange);
    }
  }
  applyTheme("system"); // 配置返回前先按系统口径，返回后以配置覆盖

  invoke("initialize_app")
    .then(function (payload) {
      var cfg = payload.config;
      symbols = cfg.symbols || [];
      applyTheme(cfg.theme);
      banner.textContent = "Rust 侧已就绪（配置来源：" + payload.source + "）";
      var problems =
        payload.validation && payload.validation.length
          ? kv("配置问题", '<span class="err">' + esc(payload.validation.join("；")) + "</span>")
          : "";
      document.getElementById("config").innerHTML =
        kv("后端地址", esc(cfg.api_url)) +
        kv("观察列表", esc(symbols.join(", "))) +
        kv("主题", esc(cfg.theme)) +
        kv("自动刷新", cfg.auto_update ? "开" : "关") +
        problems;
      refreshAlerts();
      return invoke("get_app_info");
    })
    .then(function (info) {
      document.getElementById("app-info").innerHTML =
        kv("产品", esc(info.name)) +
        kv("版本", esc(info.version)) +
        kv("平台", esc(info.os) + " / " + esc(info.arch));
      return symbols;
    })
    .then(function (list) {
      return invoke("get_real_time_quotes", { symbols: list }).then(function (quotes) {
        firstQuote = quotes && quotes.length ? quotes[0] : null;
        var rows = quotes
          .map(function (q) {
            var cls = q.price >= q.open ? "up" : "down";
            return (
              "<tr><td>" + esc(q.symbol) + '</td><td><span class="' + cls + '">' +
              num(q.price, 2) + "</span></td><td>" + esc(q.volume) + "</td></tr>"
            );
          })
          .join("");
        document.getElementById("quotes").innerHTML =
          "<table><thead><tr><th>标的</th><th>最新价</th><th>成交量</th></tr></thead><tbody>" +
          rows +
          "</tbody></table>";
        return list[0];
      });
    })
    .then(function (symbol) {
      if (!symbol) throw new Error("配置未给出任何标的");
      return invoke("analyze_symbol", {
        request: { symbol: symbol, timeframe: "1m", indicators: ["RSI", "SMA20"] },
      }).then(function (result) {
        var indicators = result.indicators
          .map(function (ind) {
            var values = ind.values || [];
            var last = values.length ? values[values.length - 1] : null;
            return kv(ind.name, num(last, 4));
          })
          .join("");
        document.getElementById("analysis").innerHTML =
          kv("标的", esc(result.symbol)) +
          kv("结论", '<span class="badge">' + esc(result.recommendation) + "</span>") +
          kv("置信度", num(result.confidence, 4)) +
          kv("波动率", num(result.risk_metrics.volatility, 4)) +
          indicators;
      });
    })
    .catch(function (error) {
      setText("analysis", error && error.message ? error.message : String(error), "err");
    });

  // L113 原生文件集成：经系统「另存为」对话框拿路径，再调 Rust 命令落盘。
  // dialog.save 是 v1 全局 API（window.__TAURI__.dialog），解析为用户选的路径，
  // 取消时为 null；后缀决定格式（.json 走 JSON，其余走 CSV，与 Rust 侧校验一致）。
  var exportBtn = document.getElementById("export-btn");
  var exportResult = document.getElementById("export-result");
  if (exportBtn && exportResult) {
    if (!invoke || !api.dialog || typeof api.dialog.save !== "function") {
      exportBtn.disabled = true;
      exportResult.textContent = "当前环境不支持原生对话框（需在 Tauri 窗口中打开）";
    } else {
      exportBtn.addEventListener("click", function () {
        var symbol = symbols[0];
        if (!symbol) {
          exportResult.textContent = "无可导出标的（配置 symbols 为空）";
          return;
        }
        exportResult.textContent = "等待选择保存位置…";
        api.dialog
          .save({
            defaultPath: symbol + ".csv",
            filters: [
              { name: "CSV", extensions: ["csv"] },
              { name: "JSON", extensions: ["json"] },
            ],
          })
          .then(function (path) {
            if (!path) {
              exportResult.textContent = "已取消";
              return null;
            }
            var fmt = /\.json$/i.test(path) ? "json" : "csv";
            return invoke("export_symbol_to_file", {
              symbol: symbol,
              format: fmt,
              filePath: path,
            });
          })
          .then(function (outcome) {
            if (outcome) {
              exportResult.textContent =
                "已导出 " + outcome.rows + " 行 → " + outcome.filename;
            }
          })
          .catch(function (error) {
            exportResult.textContent =
              "导出失败：" + (error && error.message ? error.message : String(error));
          });
      });
    }
  }

  // L114 告警/托盘链路：状态由 initialize_app 自举（State 注入）后才可调用。
  // 启动即检查一次——上次会话遗留的生效告警若已满足条件会立即触发（Rust 侧
  // 弹系统通知 + 入队 + 停用落盘）；随后把托盘 tooltip 同步为告警状态文本。
  function renderAlerts(fired) {
    document.getElementById("alert-result").innerHTML =
      fired && fired.length
        ? fired
            .map(function (n) {
              return kv("🔔 " + esc(n.title), esc(n.body));
            })
            .join("")
        : kv("本次触发", "无（布防后点按钮重查）");
  }

  function renderTrayStatus(status) {
    document.getElementById("tray-status").innerHTML =
      kv("托盘状态", esc(status.status_text)) +
      kv("生效告警", String(status.active_alerts)) +
      kv("最近触发", esc(status.last_trigger || "—"));
  }

  function refreshAlerts() {
    invoke("check_alerts")
      .then(renderAlerts)
      .then(function () {
        return invoke("set_tray_status");
      })
      .then(renderTrayStatus)
      .catch(function (error) {
        setText(
          "alert-result",
          "告警检查失败：" + (error && error.message ? error.message : String(error)),
          "err"
        );
      });
  }

  var alertBtn = document.getElementById("alert-btn");
  if (alertBtn) {
    alertBtn.addEventListener("click", function () {
      if (!firstQuote) {
        setText("alert-result", "无可布防标的（配置 symbols 为空）", "err");
        return;
      }
      alertBtn.disabled = true;
      // 演示口径：目标价取现价 −1%（确定性行情即刻满足上穿，触发即停用；
      // 同文重复触发由通知队列去重，不重复弹窗）
      invoke("set_price_alert", {
        symbol: firstQuote.symbol,
        targetPrice: firstQuote.price * 0.99,
        alertType: "above",
      })
        .then(function () {
          return invoke("check_alerts");
        })
        .then(renderAlerts)
        .then(function () {
          return invoke("set_tray_status");
        })
        .then(renderTrayStatus)
        .catch(function (error) {
          setText(
            "alert-result",
            "布防/检查失败：" + (error && error.message ? error.message : String(error)),
            "err"
          );
        })
        .then(function () {
          alertBtn.disabled = false;
        });
    });
  }
})();