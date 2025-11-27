//! WebSocket 实时数据同步
//!
//! 提供 WebSocket 连接管理和实时数据流处理

use alpha_core::models::MarketData;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, ErrorEvent, MessageEvent, WebSocket};

/// WebSocket 连接状态
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnected,
    Error,
}

/// WebSocket 消息类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WSMessage {
    /// 市场数据更新
    MarketData {
        symbol: String,
        data: MarketData,
    },
    /// 批量市场数据
    MarketDataBatch {
        symbol: String,
        data: Vec<MarketData>,
    },
    /// 订阅请求
    Subscribe {
        symbols: Vec<String>,
    },
    /// 取消订阅
    Unsubscribe {
        symbols: Vec<String>,
    },
    /// 心跳
    Ping,
    /// 心跳响应
    Pong,
    /// 错误消息
    Error {
        message: String,
    },
}

/// WebSocket 客户端
#[wasm_bindgen]
pub struct WebSocketClient {
    ws: Option<WebSocket>,
    url: String,
    state: Rc<RefCell<ConnectionState>>,
    reconnect_attempts: Rc<RefCell<usize>>,
    max_reconnect_attempts: usize,
    message_handler: Rc<RefCell<Option<js_sys::Function>>>,
}

#[wasm_bindgen]
impl WebSocketClient {
    /// 创建新的 WebSocket 客户端
    #[wasm_bindgen(constructor)]
    pub fn new(url: &str) -> WebSocketClient {
        WebSocketClient {
            ws: None,
            url: url.to_string(),
            state: Rc::new(RefCell::new(ConnectionState::Disconnected)),
            reconnect_attempts: Rc::new(RefCell::new(0)),
            max_reconnect_attempts: 5,
            message_handler: Rc::new(RefCell::new(None)),
        }
    }

    /// 连接到 WebSocket 服务器
    #[wasm_bindgen(js_name = connect)]
    pub fn connect(&mut self) -> Result<(), JsValue> {
        let ws = WebSocket::new(&self.url)
            .map_err(|e| JsValue::from_str(&format!("创建 WebSocket 失败: {:?}", e)))?;

        // 设置二进制类型为 ArrayBuffer
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        // 设置连接状态
        *self.state.borrow_mut() = ConnectionState::Connecting;

        // 克隆用于闭包的引用
        let state_clone = self.state.clone();
        let reconnect_attempts = self.reconnect_attempts.clone();
        let _message_handler = self.message_handler.clone();

        // onopen 回调
        let onopen = Closure::wrap(Box::new(move || {
            *state_clone.borrow_mut() = ConnectionState::Connected;
            *reconnect_attempts.borrow_mut() = 0;
            web_sys::console::log_1(&JsValue::from_str("WebSocket 已连接"));
        }) as Box<dyn FnMut()>);

        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();

        // onmessage 回调
        let _state_clone = self.state.clone();
        let message_handler_clone = self.message_handler.clone();

        let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Ok(text) = event.data().dyn_into::<js_sys::JsString>() {
                let text_str = String::from(text);

                // 调用用户注册的消息处理函数
                if let Some(handler) = message_handler_clone.borrow().as_ref() {
                    let _ = handler.call1(&JsValue::NULL, &JsValue::from_str(&text_str));
                }

                web_sys::console::log_1(&JsValue::from_str(&format!(
                    "收到消息: {}",
                    text_str
                )));
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        // onerror 回调
        let state_clone = self.state.clone();

        let onerror = Closure::wrap(Box::new(move |_event: ErrorEvent| {
            *state_clone.borrow_mut() = ConnectionState::Error;
            web_sys::console::error_1(&JsValue::from_str("WebSocket 错误"));
        }) as Box<dyn FnMut(ErrorEvent)>);

        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        // onclose 回调
        let state_clone = self.state.clone();
        let reconnect_attempts_clone = self.reconnect_attempts.clone();
        let max_attempts = self.max_reconnect_attempts;

        let onclose = Closure::wrap(Box::new(move |_event: CloseEvent| {
            *state_clone.borrow_mut() = ConnectionState::Disconnected;

            let mut attempts = reconnect_attempts_clone.borrow_mut();
            if *attempts < max_attempts {
                *attempts += 1;
                web_sys::console::log_1(&JsValue::from_str(&format!(
                    "WebSocket 已断开，将尝试重连 ({}/{})",
                    *attempts, max_attempts
                )));
                // 这里可以触发重连逻辑
            } else {
                web_sys::console::error_1(&JsValue::from_str("达到最大重连次数"));
            }
        }) as Box<dyn FnMut(CloseEvent)>);

        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

        self.ws = Some(ws);
        Ok(())
    }

    /// 断开连接
    #[wasm_bindgen(js_name = disconnect)]
    pub fn disconnect(&mut self) -> Result<(), JsValue> {
        if let Some(ws) = &self.ws {
            ws.close()
                .map_err(|e| JsValue::from_str(&format!("关闭 WebSocket 失败: {:?}", e)))?;
            *self.state.borrow_mut() = ConnectionState::Disconnected;
        }
        Ok(())
    }

    /// 发送消息
    #[wasm_bindgen(js_name = send)]
    pub fn send(&self, message: &str) -> Result<(), JsValue> {
        if let Some(ws) = &self.ws {
            if *self.state.borrow() == ConnectionState::Connected {
                ws.send_with_str(message)
                    .map_err(|e| JsValue::from_str(&format!("发送消息失败: {:?}", e)))?;
                Ok(())
            } else {
                Err(JsValue::from_str("WebSocket 未连接"))
            }
        } else {
            Err(JsValue::from_str("WebSocket 未初始化"))
        }
    }

    /// 订阅股票数据
    #[wasm_bindgen(js_name = subscribe)]
    pub fn subscribe(&self, symbols: js_sys::Array) -> Result<(), JsValue> {
        let symbol_list: Vec<String> = symbols
            .iter()
            .filter_map(|s| s.as_string())
            .collect();

        let message = WSMessage::Subscribe {
            symbols: symbol_list,
        };

        let json = serde_json::to_string(&message)
            .map_err(|e| JsValue::from_str(&format!("序列化失败: {}", e)))?;

        self.send(&json)
    }

    /// 取消订阅
    #[wasm_bindgen(js_name = unsubscribe)]
    pub fn unsubscribe(&self, symbols: js_sys::Array) -> Result<(), JsValue> {
        let symbol_list: Vec<String> = symbols
            .iter()
            .filter_map(|s| s.as_string())
            .collect();

        let message = WSMessage::Unsubscribe {
            symbols: symbol_list,
        };

        let json = serde_json::to_string(&message)
            .map_err(|e| JsValue::from_str(&format!("序列化失败: {}", e)))?;

        self.send(&json)
    }

    /// 发送心跳
    #[wasm_bindgen(js_name = sendPing)]
    pub fn send_ping(&self) -> Result<(), JsValue> {
        let message = WSMessage::Ping;
        let json = serde_json::to_string(&message)
            .map_err(|e| JsValue::from_str(&format!("序列化失败: {}", e)))?;
        self.send(&json)
    }

    /// 设置消息处理函数
    #[wasm_bindgen(js_name = onMessage)]
    pub fn on_message(&mut self, handler: js_sys::Function) {
        *self.message_handler.borrow_mut() = Some(handler);
    }

    /// 获取连接状态
    #[wasm_bindgen(js_name = getState)]
    pub fn get_state(&self) -> String {
        format!("{:?}", *self.state.borrow())
    }

    /// 是否已连接
    #[wasm_bindgen(js_name = isConnected)]
    pub fn is_connected(&self) -> bool {
        *self.state.borrow() == ConnectionState::Connected
    }

    /// 获取重连次数
    #[wasm_bindgen(js_name = getReconnectAttempts)]
    pub fn get_reconnect_attempts(&self) -> usize {
        *self.reconnect_attempts.borrow()
    }
}

/// WebSocket 连接池（管理多个连接）
#[wasm_bindgen]
pub struct WebSocketPool {
    connections: std::collections::HashMap<String, WebSocketClient>,
    default_url: String,
}

#[wasm_bindgen]
impl WebSocketPool {
    /// 创建连接池
    #[wasm_bindgen(constructor)]
    pub fn new(default_url: &str) -> WebSocketPool {
        WebSocketPool {
            connections: std::collections::HashMap::new(),
            default_url: default_url.to_string(),
        }
    }

    /// 添加连接
    #[wasm_bindgen(js_name = addConnection)]
    pub fn add_connection(&mut self, name: &str, url: Option<String>) -> Result<(), JsValue> {
        let connection_url = url.unwrap_or_else(|| self.default_url.clone());
        let mut client = WebSocketClient::new(&connection_url);
        client.connect()?;

        self.connections.insert(name.to_string(), client);
        Ok(())
    }

    /// 移除连接
    #[wasm_bindgen(js_name = removeConnection)]
    pub fn remove_connection(&mut self, name: &str) -> Result<(), JsValue> {
        if let Some(mut client) = self.connections.remove(name) {
            client.disconnect()?;
        }
        Ok(())
    }

    /// 获取连接数
    #[wasm_bindgen(js_name = getConnectionCount)]
    pub fn get_connection_count(&self) -> usize {
        self.connections.len()
    }

    /// 广播消息到所有连接
    #[wasm_bindgen(js_name = broadcast)]
    pub fn broadcast(&self, message: &str) -> Result<(), JsValue> {
        for client in self.connections.values() {
            client.send(message)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_client_creation() {
        let client = WebSocketClient::new("ws://localhost:8080");
        assert_eq!(client.url, "ws://localhost:8080");
        assert!(!client.is_connected());
    }

    #[test]
    fn test_websocket_pool() {
        let pool = WebSocketPool::new("ws://localhost:8080");
        assert_eq!(pool.get_connection_count(), 0);
    }

    #[test]
    fn test_message_serialization() {
        let message = WSMessage::Subscribe {
            symbols: vec!["AAPL".to_string(), "GOOGL".to_string()],
        };

        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("Subscribe"));
        assert!(json.contains("AAPL"));
    }
}
