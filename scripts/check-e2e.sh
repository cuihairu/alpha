#!/usr/bin/env bash
# 端到端跨服务冒烟测试（TODO L469，方案见 docs/docker-deployment.md §4 与 TODO 标注）
#
# 场景：以真实进程拉起 data-engine / real-time-feed / collector / api-gateway 四服务，
# 走跨服务链路断言：
#   1. 各服务 /health 自身可用
#   2. gateway 聚合健康探测三上游全 healthy
#   3. REST 反代 gateway → data-engine（/api/v1/health）+ x-trace-id 回填
#   4. WS 反代 gateway → real-time-feed（101 Upgrade）
#   5. gateway /metrics 暴露业务指标（requests_total / service_health）
#   6. 数据面 e2e：XADD 行情（examples/stream_inject，生产同构 envelope）
#      → real-time-feed 消费组 → 广播 → 网关 /ws 客户端收敛到注入价格
#
# 依赖：Redis（data-engine/real-time-feed 启动即连，E2E_REDIS_URL 可覆写）、
#       cargo（构建四服务二进制）。非交互；退出码 0 通过 / 1 失败。
set -euo pipefail

cd "$(dirname "$0")/.."
export PATH="${HOME}/.cargo/bin:${PATH}"

GW_PORT="${E2E_GATEWAY_PORT:-18080}"
DE_PORT="${E2E_DATA_ENGINE_PORT:-18081}"
GRPC_PORT="${E2E_GRPC_PORT:-15051}"
# real-time-feed 绑定 8082：HEAD 版代码硬编码（可覆写 env 随其 L46x 批次落库后自动生效）
RT_PORT="${E2E_REALTIME_PORT:-8082}"
COL_PORT="${E2E_COLLECTOR_PORT:-18083}"
REDIS_URL="${E2E_REDIS_URL:-redis://127.0.0.1:6379}"

BIN=target/debug
LOG_DIR="${E2E_LOG_DIR:-/tmp/opencode/alpha-e2e-$$}"
mkdir -p "$LOG_DIR"
PIDS=()

fail() {
  echo "❌ E2E 失败: $*" >&2
  for f in "$LOG_DIR"/*.log; do
    [ -f "$f" ] || continue
    echo "--- $(basename "$f") 末 30 行 ---" >&2
    tail -30 "$f" >&2 || true
  done
  exit 1
}

port_free() {
  ! (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null
}

cleanup() {
  local pid
  for pid in "${PIDS[@]:-}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true
}
trap cleanup EXIT

# --- 前置：Redis 可达（带重试，兼容 CI 容器冷启）+ 端口空闲 ---
redis_ready=0
for _ in $(seq 1 15); do
  if (exec 3<>/dev/tcp/127.0.0.1/"${REDIS_URL##*:}") 2>/dev/null; then
    redis_ready=1
    break
  fi
  sleep 1
done
[ "$redis_ready" = "1" ] || fail "Redis 不可达（${REDIS_URL}）：先起本地 Redis（CI 由 docker/brew 提供）"
for p in "$GW_PORT" "$DE_PORT" "$GRPC_PORT" "$RT_PORT" "$COL_PORT"; do
  port_free "$p" || fail "端口 $p 已被占用（残留 dev 服务？先停再跑 e2e）"
done

# --- 构建四服务（缓存命中时近零成本，保证二进制与当前源码一致） ---
echo "==> cargo build（四服务 + 注入 example）"
cargo build -p alpha-api-gateway -p alpha-data-engine -p alpha-real-time-feed -p alpha-collector \
  || fail "cargo build 失败"
# 注入端一并预建：断言 3c 的竞速窗口内直跑二进制，不吃 cargo run 的
# workspace 新鲜度检查（实测 ~10s，足以错过客户端超时窗口）
cargo build -p alpha-storage --example stream_inject || fail "stream_inject 构建失败"

# --- 启动四服务 ---
echo "==> 启动服务（logs: $LOG_DIR）"
ALPHA__SERVER__ADDR="127.0.0.1:$DE_PORT" \
ALPHA__SERVER__GRPC_ADDR="127.0.0.1:$GRPC_PORT" \
ALPHA__STORAGE__PERSISTENCE_ENABLED=false \
REDIS_URL="$REDIS_URL" "$BIN/alpha-data-engine" >"$LOG_DIR/data-engine.log" 2>&1 &
PIDS+=($!)

REDIS_URL="$REDIS_URL" "$BIN/alpha-real-time-feed" >"$LOG_DIR/real-time-feed.log" 2>&1 &
PIDS+=($!)

ALPHA_COLLECTOR_BIND="127.0.0.1:$COL_PORT" \
REDIS_URL="$REDIS_URL" "$BIN/alpha-collector" >"$LOG_DIR/collector.log" 2>&1 &
PIDS+=($!)

ALPHA_GATEWAY_DATA_ENGINE_URL="http://127.0.0.1:$DE_PORT" \
ALPHA_GATEWAY_REALTIME_URL="http://127.0.0.1:$RT_PORT" \
ALPHA_GATEWAY_COLLECTOR_URL="http://127.0.0.1:$COL_PORT" \
"$BIN/alpha-api-gateway" --bind "127.0.0.1:$GW_PORT" >"$LOG_DIR/api-gateway.log" 2>&1 &
PIDS+=($!)

# --- 等待各服务 /health 就绪（逐个最长 30s） ---
wait_health() {
  local name="$1" url="$2"
  for _ in $(seq 1 30); do
    if [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 2 "$url" || true)" = "200" ]; then
      echo "   $name 就绪"
      return 0
    fi
    sleep 1
  done
  fail "$name 30s 内未就绪（$url）"
}
echo "==> 等待 /health"
wait_health data-engine "http://127.0.0.1:$DE_PORT/health"
wait_health real-time-feed "http://127.0.0.1:$RT_PORT/health"
wait_health collector "http://127.0.0.1:$COL_PORT/health"
wait_health api-gateway "http://127.0.0.1:$GW_PORT/health"

# --- 断言 1：gateway 聚合探测三上游全 healthy ---
body="$(curl -s --max-time 5 "http://127.0.0.1:$GW_PORT/health")"
healthy_n="$({ grep -o '"healthy"' <<<"$body" || true; } | wc -l | tr -d ' ')"
[ "$healthy_n" -ge 3 ] || fail "gateway 聚合健康未全绿（healthy=$healthy_n/3）：$body"
echo "✅ 聚合健康：三上游 healthy=$healthy_n/3"

# --- 断言 2：REST 反代 gateway → data-engine + x-trace-id 回填 ---
resp="$(curl -s -i --max-time 5 "http://127.0.0.1:$GW_PORT/api/v1/health")"
grep -q '200' <<<"$(printf '%s' "$resp" | head -1)" || fail "REST 反代非 200：$(printf '%s' "$resp" | head -3)"
grep -qi '^x-trace-id:' <<<"$resp" || fail "REST 反代响应缺 x-trace-id 回填"
echo "✅ REST 反代：/api/v1/health 200 + x-trace-id 回填"

# --- 断言 3：WS 反代 gateway → real-time-feed 握手 101 ---
ws_code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 \
  -H 'Connection: Upgrade' -H 'Upgrade: websocket' \
  -H 'Sec-WebSocket-Version: 13' -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' \
  "http://127.0.0.1:$GW_PORT/ws" || true)"
[ "$ws_code" = "101" ] || fail "WS 反代握手非 101（got=$ws_code）"
echo "✅ WS 反代：/ws 握手 101"

# --- 断言 3b：WS 消息级契约——版本化同步 Resync 回帧（L491 统筹补深） ---
# L469 登记的缺口「未含行情消息级 WS 载荷断言」的最小闭环：连上 /ws 后发
# Subscribe（登记订阅）+ Resync（from_seq=0 丢帧恢复），契约保证「无论通道
# 是否有数据都必须回帧」（Full 快照或 Error），超时静默 = 协议破坏。
# 数据面注入（XADD 行情 → 广播 Delta 断言）依赖 envelope 解析链，留待
# 数据面 e2e 加深时接（见 docs/testing.md §边界）。
WS_SCRIPT="$(mktemp /tmp/ws_resync_XXXX.mjs)"
cat >"$WS_SCRIPT" <<'EOF'
const url = process.argv[2];
const ws = new WebSocket(url);
const timeout = setTimeout(() => {
  console.error('❌ WS 消息级：5s 内未收到任何回帧（Resync 静默 = 协议破坏）');
  process.exit(1);
}, 5000);
const fail = (msg) => { clearTimeout(timeout); console.error(`❌ WS 消息级：${msg}`); process.exit(1); };
ws.onopen = () => {
  // 线上帧型 = serde variant 原名 PascalCase（无 rename，实测探针锁定）：
  // {"type":"Resync",...} / {"type":"Sync",...}——小写会静默不进分支
  ws.send(JSON.stringify({ type: 'Subscribe', id: 'e2e-l491', channels: ['real_time_quotes'], symbols: ['600519'] }));
  ws.send(JSON.stringify({ type: 'Resync', channel: 'real_time_quotes', from_seq: 0 }));
};
const seen = [];
ws.onmessage = (ev) => {
  let frame;
  try { frame = JSON.parse(ev.data); } catch { fail(`非 JSON 帧: ${String(ev.data).slice(0, 80)}`); return; }
  seen.push(frame.type);
  if (frame.type === 'Sync') {
    // SyncMessage 契约：channel 回显 + seq 数值 + op ∈ {full, delta} + data 在位
    // op 用大小写不敏感匹配：线上帧是 serde variant 原名 PascalCase（"Full"/"Delta"，无 rename_all）
    if (frame.channel !== 'real_time_quotes') fail(`sync.channel 未回显: ${frame.channel}`);
    if (typeof frame.seq !== 'number') fail(`sync.seq 非数值: ${frame.seq}`);
    if (!/^(full|delta)$/i.test(String(frame.op))) fail(`sync.op 非法: ${frame.op}`);
    if (frame.data === undefined) fail('sync.data 缺失');
    clearTimeout(timeout);
    console.log(`✅ WS 消息级：Resync → ${frame.op} 快照回帧（seq=${frame.seq}，全程帧型: ${seen.join(',')})`);
    process.exit(0);
  }
  if (frame.type === 'Error') {
    // 通道不存在等：契约允许 Error 回帧（必须显式拒绝而非静默）
    if (typeof frame.code !== 'number' || typeof frame.message !== 'string') fail('error 帧缺 code/message');
    clearTimeout(timeout);
    console.log(`✅ WS 消息级：Resync → 显式 Error 回帧（code=${frame.code}，全程帧型: ${seen.join(',')})`);
    process.exit(0);
  }
  // 其他帧型（Connected/Ping 等）：继续等 Sync/Error
};
ws.onerror = () => fail('连接错误');
EOF
node "$WS_SCRIPT" "ws://127.0.0.1:$GW_PORT/ws" || fail "WS 消息级 Resync 契约断言失败"
rm -f "$WS_SCRIPT"
echo "✅ WS 消息级：版本化同步 Resync 回帧契约成立"

# --- 断言 3c：数据面 e2e——XADD 行情 → real-time-feed 消费 → /ws 客户端收敛 ---
# （testing.md §4 登记的「数据面 e2e」缺口闭合）：注入端走
# examples/stream_inject.rs（与生产同一 publish 组装面，非手拼 JSON），
# 断言端以真实 WS 客户端等待 Sync 帧中的注入价格——envelope 解析链
# （消费组读取 → envelope_to_realtime → 版本化广播）任何一环断即超时失败。
WS_INJECT_SCRIPT="$(mktemp /tmp/ws_inject_XXXX.mjs)"
cat >"$WS_INJECT_SCRIPT" <<'EOF'
const url = process.argv[2];
const expectPrice = Number(process.argv[3]);
const fail = (msg) => { console.error(`❌ 数据面 e2e：${msg}`); process.exit(1); };
const ws = new WebSocket(url);
const timeout = setTimeout(() => fail('10s 内未收到含注入价格的 Sync 帧（解析/广播链断裂）'), 10000);
ws.onopen = () => {
  ws.send(JSON.stringify({ type: 'Subscribe', id: 'e2e-data', channels: ['real_time_quotes'], symbols: ['600519'] }));
};
ws.onmessage = (ev) => {
  let frame;
  try { frame = JSON.parse(ev.data); } catch { fail(`非 JSON 帧: ${String(ev.data).slice(0, 80)}`); return; }
  if (frame.type === 'Sync' && frame.data && Number(frame.data.price) === expectPrice) {
    clearTimeout(timeout);
    console.log(`✅ 数据面：注入价格 ${expectPrice} 经 ${frame.op} 帧收敛（seq=${frame.seq}）`);
    process.exit(0);
  }
  // 其他帧（Connected / 无关 Sync）：继续等
};
ws.onerror = () => fail('连接错误');
EOF
node "$WS_INJECT_SCRIPT" "ws://127.0.0.1:$GW_PORT/ws" 13.37 &
WS_INJECT_PID=$!
sleep 1.5
# 注入：与生产同构的 envelope（RedisStreamQueue::publish 组装）；竞速窗口
# 1.5s 已含订阅落定 + 组消费派发（直跑预建二进制，秒级内落库），若上游
# 消费断链，客户端 10s 超时兜底报红
inject_out="$("$BIN/examples/stream_inject" "$REDIS_URL" quotes.raw 600519 13.37 \
  2>"$LOG_DIR/stream_inject.log")" \
  || { kill "$WS_INJECT_PID" 2>/dev/null || true; fail "行情注入失败（见 stream_inject.log）"; }
wait "$WS_INJECT_PID" || fail "WS 客户端未收敛到注入价格（注入 id=${inject_out:-?}）"
rm -f "$WS_INJECT_SCRIPT"
echo "✅ 数据面 e2e：XADD 行情 → 广播 → /ws 收敛（price=13.37）"

# --- 断言 4：gateway /metrics 暴露业务指标（跨服务调用已发生之后） ---
gw_metrics="$(curl -s --max-time 5 "http://127.0.0.1:$GW_PORT/metrics")"
grep -q '^alpha_gateway_requests_total' <<<"$gw_metrics" \
  || fail "/metrics 缺 alpha_gateway_requests_total（metrics slot 回归？）"
grep -q 'alpha_gateway_service_health{upstream="data-engine"} 1' <<<"$gw_metrics" \
  || fail "/metrics 缺 alpha_gateway_service_health{upstream=\"data-engine\"}"
echo "✅ gateway 业务指标在位（requests_total + service_health）"

# --- 断言 5：四服务 /metrics 端点全部可抓取 ---
for p in "$DE_PORT" "$RT_PORT" "$COL_PORT"; do
  code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "http://127.0.0.1:$p/metrics" || true)"
  [ "$code" = "200" ] || fail "127.0.0.1:$p/metrics 非 200（got=$code）"
done
echo "✅ data-engine / real-time-feed / collector /metrics 均 200"

echo "✅ E2E 全部通过（gateway=$GW_PORT data-engine=$DE_PORT realtime=$RT_PORT collector=$COL_PORT redis=$REDIS_URL）"
