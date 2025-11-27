//! 性能基准测试
//!
//! 测试各个模块的性能表现

#![cfg(test)]

use alpha_wasm_analyzer::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn bench_sma_calculation() {
    let analyzer = WasmAnalyzer::new(None);

    // 生成测试数据
    let mut prices = Vec::new();
    for i in 0..10000 {
        prices.push(100.0 + (i as f64 * 0.1));
    }

    let prices_array = js_sys::Float64Array::from(&prices[..]);

    // 测试 SMA 计算性能
    let start = web_sys::window()
        .unwrap()
        .performance()
        .unwrap()
        .now();

    let _result = analyzer.calculate_sma(&prices_array, 20);

    let end = web_sys::window()
        .unwrap()
        .performance()
        .unwrap()
        .now();

    let duration = end - start;
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&format!(
        "SMA(20) 计算 10000 数据点耗时: {:.2}ms",
        duration
    )));

    assert!(duration < 100.0, "SMA 计算应在 100ms 内完成");
}

#[wasm_bindgen_test]
fn bench_rsi_calculation() {
    let analyzer = WasmAnalyzer::new(None);

    let mut prices = Vec::new();
    for i in 0..10000 {
        prices.push(100.0 + (i as f64 * 0.1));
    }

    let prices_array = js_sys::Float64Array::from(&prices[..]);

    let start = web_sys::window()
        .unwrap()
        .performance()
        .unwrap()
        .now();

    let _result = analyzer.calculate_rsi(&prices_array, 14);

    let end = web_sys::window()
        .unwrap()
        .performance()
        .unwrap()
        .now();

    let duration = end - start;
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&format!(
        "RSI(14) 计算 10000 数据点耗时: {:.2}ms",
        duration
    )));

    assert!(duration < 100.0, "RSI 计算应在 100ms 内完成");
}

#[wasm_bindgen_test]
fn bench_all_indicators() {
    let analyzer = WasmAnalyzer::new(None);

    let mut prices = Vec::new();
    for i in 0..10000 {
        prices.push(100.0 + (i as f64 * 0.1));
    }

    let prices_array = js_sys::Float64Array::from(&prices[..]);

    let start = web_sys::window()
        .unwrap()
        .performance()
        .unwrap()
        .now();

    let _result = analyzer.calculate_all_indicators(
        &prices_array,
        14, // rsi_period
        5,  // sma_short
        20, // sma_long
        12, // macd_fast
        26, // macd_slow
        9,  // macd_signal
    );

    let end = web_sys::window()
        .unwrap()
        .performance()
        .unwrap()
        .now();

    let duration = end - start;
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&format!(
        "批量计算所有指标 10000 数据点耗时: {:.2}ms",
        duration
    )));

    assert!(duration < 200.0, "批量指标计算应在 200ms 内完成");
}

#[wasm_bindgen_test]
fn bench_stream_processor() {
    let mut processor = StreamProcessor::new(1000);

    let start = web_sys::window()
        .unwrap()
        .performance()
        .unwrap()
        .now();

    // 模拟流式数据处理
    for i in 0..1000 {
        let data = alpha_core::models::MarketData::new(
            "AAPL".to_string(),
            100.0 + i as f64,
            1000 + i,
        );

        let data_js = serde_wasm_bindgen::to_value(&data).unwrap();
        processor.push_data(&data_js).unwrap();
    }

    let end = web_sys::window()
        .unwrap()
        .performance()
        .unwrap()
        .now();

    let duration = end - start;
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&format!(
        "流式处理 1000 数据点耗时: {:.2}ms",
        duration
    )));

    assert!(duration < 50.0, "流式处理应在 50ms 内完成");
    assert_eq!(processor.get_buffer_size(), 1000);
}

#[wasm_bindgen_test]
fn bench_batch_computer() {
    let computer = BatchComputer::new(100);

    let mut prices = Vec::new();
    for i in 0..5000 {
        prices.push(100.0 + (i as f64 * 0.1));
    }

    let prices_array = js_sys::Float64Array::from(&prices[..]);

    let start = web_sys::window()
        .unwrap()
        .performance()
        .unwrap()
        .now();

    let _result = computer
        .batch_compute_multiple(prices_array, 20, 12, 14)
        .unwrap();

    let end = web_sys::window()
        .unwrap()
        .performance()
        .unwrap()
        .now();

    let duration = end - start;
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&format!(
        "批量计算器处理 5000 数据点耗时: {:.2}ms",
        duration
    )));

    assert!(duration < 150.0, "批量计算应在 150ms 内完成");
}

#[wasm_bindgen_test]
fn bench_memory_usage() {
    let analyzer = WasmAnalyzer::new(None);

    // 测试大数据集内存使用
    let mut prices = Vec::new();
    for i in 0..100000 {
        prices.push(100.0 + (i as f64 * 0.1));
    }

    let prices_array = js_sys::Float64Array::from(&prices[..]);

    let memory_before = current_memory_usage_bytes();

    let _result = analyzer.calculate_sma(&prices_array, 20);

    let memory_after = current_memory_usage_bytes();

    let memory_used = memory_after.saturating_sub(memory_before);

    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&format!(
        "处理 100000 数据点内存增长: {} bytes ({:.2} MB)",
        memory_used,
        memory_used as f64 / 1024.0 / 1024.0
    )));
}

fn current_memory_usage_bytes() -> u32 {
    wasm_bindgen::memory()
        .dyn_into::<js_sys::WebAssembly::Memory>()
        .map(|memory| {
            let buffer = js_sys::Uint8Array::new(&memory.buffer());
            buffer.length()
        })
        .unwrap_or(0)
}
