/**
 * 桌面骨架兜底壳的前端逻辑（TODO L112）。
 *
 * 只用 Tauri v1 全局 API（window.__TAURI__，由 tauri.conf.json 的 build.withGlobalTauri
 * 注入；v1 全局 API 是 window.__TAURI__.invoke(cmd, args)，没有 v2 的 ipcRenderer），
 * 不引入 npm 依赖 —— 保证 distDir 只有这两个文件时窗口依然可用。四条命令对应
 * desktop/src/gui.rs 的 #[tauri::command]：
 *   initialize_app → get_app_info → get_real_time_quotes → analyze_symbol
 * 链路终点是 alpha-core 的 AnalysisEngine（真实计算，非桩数据）。
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

  invoke("initialize_app")
    .then(function (payload) {
      var cfg = payload.config;
      symbols = cfg.symbols || [];
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
})();