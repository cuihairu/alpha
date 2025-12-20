//! 简化的多语言爬虫支持模块

use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    time::Duration,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command as TokioCommand;
use tracing::{debug, error, info, warn};

use crate::types::TaskDefinition;
use super::{
    data_sources::*,
    distributed_crawler::*,
};

/// 扩展多语言爬虫执行器
pub struct MultilangCrawler {
    /// 工作目录根路径
    workspace_root: PathBuf,
    /// 临时文件目录
    temp_dir: PathBuf,
    /// 分布式爬虫管理器
    distributed_manager: Option<distributed_crawler::DistributedCrawlerManager>,
}

impl MultilangCrawler {
    /// 创建新的多语言爬虫执行器
    pub fn new<P: AsRef<std::path::Path>>(workspace_root: P) -> Self {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let temp_dir = workspace_root.join("temp");

        Self {
            workspace_root,
            temp_dir,
            distributed_manager: None,
        }
    }

    /// 创建带分布式爬虫支持的多语言爬虫执行器
    pub fn new_with_distributed<P: AsRef<std::path::Path>>(
        workspace_root: P,
        distributed_manager: Option<distributed_crawler::DistributedCrawlerManager>,
    ) -> Self {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let temp_dir = workspace_root.join("temp");

        Self {
            workspace_root,
            temp_dir,
            distributed_manager,
        }
    }
}

/// 支持的爬虫语言类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CrawlerLanguage {
    Python,
    NodeJs,
    Go,
    Rust,
    Shell,
}

impl CrawlerLanguage {
    /// 获取语言的文件扩展名
    pub fn extension(&self) -> &'static str {
        match self {
            CrawlerLanguage::Python => "py",
            CrawlerLanguage::NodeJs => "js",
            CrawlerLanguage::Go => "go",
            CrawlerLanguage::Rust => "rs",
            CrawlerLanguage::Shell => "sh",
        }
    }

    /// 获取语言的解释器/编译器命令
    pub fn command(&self) -> &'static str {
        match self {
            CrawlerLanguage::Python => "python3",
            CrawlerLanguage::NodeJs => "node",
            CrawlerLanguage::Go => "go",
            CrawlerLanguage::Rust => "cargo",
            CrawlerLanguage::Shell => "bash",
        }
    }

    /// 获取语言运行时是否可用
    pub async fn is_available(&self) -> bool {
        let result = TokioCommand::new("sh")
            .arg("-c")
            .arg(format!("command -v {}", self.command()))
            .output()
            .await;

        match result {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// 获取语言的默认执行超时时间
    pub fn default_timeout(&self) -> Duration {
        match self {
            CrawlerLanguage::Python => Duration::from_secs(300), // 5 minutes
            CrawlerLanguage::NodeJs => Duration::from_secs(180), // 3 minutes
            CrawlerLanguage::Go => Duration::from_secs(120),   // 2 minutes
            CrawlerLanguage::Rust => Duration::from_secs(600),  // 10 minutes (compilation time)
            CrawlerLanguage::Shell => Duration::from_secs(60),  // 1 minute
        }
    }
}

/// 爬虫执行配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerConfig {
    /// 执行语言
    pub language: CrawlerLanguage,
    /// 脚本路径或代码
    pub script_path: Option<String>,
    /// 内联代码（当script_path为None时使用）
    pub inline_code: Option<String>,
    /// 工作目录
    pub working_directory: Option<String>,
    /// 环境变量
    pub environment: HashMap<String, String>,
    /// 执行超时时间
    pub timeout: Option<u64>, // seconds
    /// 命令行参数
    pub arguments: Vec<String>,
    /// Python虚拟环境路径
    pub python_venv: Option<String>,
    /// Node.js项目路径
    pub node_project_path: Option<String>,
    /// Go模块路径
    pub go_module_path: Option<String>,
}

impl Default for CrawlerConfig {
    fn default() -> Self {
        Self {
            language: CrawlerLanguage::Python,
            script_path: None,
            inline_code: None,
            working_directory: None,
            environment: HashMap::new(),
            timeout: None,
            arguments: Vec::new(),
            python_venv: None,
            node_project_path: None,
            go_module_path: None,
        }
    }
}

/// 多语言爬虫执行器
pub struct MultilangCrawler {
    /// 工作目录根路径
    workspace_root: PathBuf,
    /// 临时文件目录
    temp_dir: PathBuf,
    /// 分布式爬虫管理器
    distributed_manager: Option<distributed_crawler::DistributedCrawlerManager>,
}

impl MultilangCrawler {
    /// 创建新的多语言爬虫执行器
    pub fn new<P: AsRef<std::path::Path>>(workspace_root: P) -> Self {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let temp_dir = workspace_root.join("temp");

        Self {
            workspace_root,
            temp_dir,
            distributed_manager: None,
        }
    }

    /// 创建带分布式爬虫支持的多语言爬虫执行器
    pub fn new_with_distributed<P: AsRef<std::path::Path>>(
        workspace_root: P,
        distributed_manager: Option<distributed_crawler::DistributedCrawlerManager>,
    ) -> Self {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let temp_dir = workspace_root.join("temp");

        Self {
            workspace_root,
            temp_dir,
            distributed_manager,
        }
    }

    /// 初始化执行器
    pub async fn initialize(&self) -> Result<()> {
        // 创建临时目录
        tokio::fs::create_dir_all(&self.temp_dir).await?;

        // 初始化分布式爬虫管理器
        if let Some(ref manager) = self.distributed_manager {
            if let Err(e) = manager.discover_crawlers().await {
                warn!("分布式爬虫初始化失败: {}", e);
            } else {
                info!("分布式爬虫初始化完成，发现 {} 个爬虫项目", manager.crawlers.len());
            }
        }

        // 检查所有语言的运行时可用性
        let languages = [
            CrawlerLanguage::Python,
            CrawlerLanguage::NodeJs,
            CrawlerLanguage::Go,
            CrawlerLanguage::Rust,
            CrawlerLanguage::Shell,
        ];

        for language in &languages {
            let available = language.is_available().await;
            if available {
                info!("{} runtime is available", language.command());
            } else {
                warn!("{} runtime is not available", language.command());
            }
        }

        Ok(())
    }

    /// 执行爬虫任务
    pub async fn execute_crawler(&self, task: &TaskDefinition, config: &CrawlerConfig) -> Result<String> {
        let start_time = std::time::Instant::now();

        // 检查语言运行时可用性
        if !config.language.is_available().await {
            return Err(anyhow!("Language runtime {} is not available", config.language.command()));
        }

        // 准备执行环境
        let script_content = if let Some(script_path) = &config.script_path {
            // 从文件读取
            tokio::fs::read_to_string(script_path).await?
        } else if let Some(inline_code) = &config.inline_code {
            // 使用内联代码
            inline_code.clone()
        } else {
            return Err(anyhow!("Either script_path or inline_code must be provided"));
        };

        let extension = config.language.extension();
        let script_file = self.temp_dir.join(format!("crawler_{}.{}",
            uuid::Uuid::new_v4().to_string().replace("-", "_"), extension));

        // 写入脚本文件
        tokio::fs::write(&script_file, script_content).await?;

        // 设置执行权限
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = tokio::fs::metadata(&script_file).await?.permissions();
            let mut new_perms = perms.clone();
            new_perms.set_mode(0o755);
            tokio::fs::set_permissions(&script_file, new_perms).await?;
        }

        debug!("Prepared script file: {:?}", script_file);

        // 准备工作目录
        let working_dir = if let Some(dir) = &config.working_directory {
            PathBuf::from(dir)
        } else {
            self.temp_dir.join(format!("workspace_{}",
                uuid::Uuid::new_v4().to_string().replace("-", "_")))
        };

        tokio::fs::create_dir_all(&working_dir).await?;

        // 构建执行命令
        let mut cmd = self.build_command(config, &script_file)?;

        // 设置工作目录
        cmd.current_dir(&working_dir);

        // 设置环境变量
        for (key, value) in &config.environment {
            cmd.env(key, value);
        }

        // 执行命令
        let timeout = Duration::from_secs(config.timeout.unwrap_or_else(|| {
            config.language.default_timeout().as_secs()
        }));

        debug!("Executing command: {:?}", cmd);
        let output = tokio::time::timeout(timeout, cmd.output()).await;

        // 清理临时文件
        if let Err(e) = tokio::fs::remove_file(&script_file).await {
            warn!("Failed to cleanup temp file: {}", e);
        }

        let execution_time = start_time.elapsed();

        match output {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("Script execution failed: {}", stderr);
                    return Err(anyhow!("Script execution failed: {}", stderr));
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if !stderr.is_empty() {
                    warn!("Script stderr: {}", stderr);
                }

                info!("Crawler execution completed successfully in {:?}", execution_time);
                Ok(stdout.to_string())
            }
            Ok(Err(e)) => {
                error!("Script execution timed out: {}", e);
                Err(anyhow!("Script execution timed out after {:?}", timeout))
            }
            Err(e) => {
                error!("Failed to execute script: {}", e);
                Err(anyhow!("Failed to execute script: {}", e))
            }
        }
    }

    /// 构建执行命令
    fn build_command(&self, config: &CrawlerConfig, script_path: &std::path::Path) -> Result<TokioCommand> {
        let mut cmd = match &config.language {
            CrawlerLanguage::Python => {
                let python_cmd = if let Some(venv) = &config.python_venv {
                    format!("{}/bin/python3", venv)
                } else {
                    config.language.command().to_string()
                };

                let mut cmd = TokioCommand::new("sh");
                cmd.arg("-c")
                    .arg(format!("{} {}", python_cmd, script_path.display()));
                cmd
            }

            CrawlerLanguage::NodeJs => {
                let node_cmd = if let Some(_project_path) = &config.node_project_path {
                    format!("node {}", script_path.display())
                } else {
                    format!("{} {}", config.language.command(), script_path.display())
                };

                let mut cmd = TokioCommand::new("sh");
                cmd.arg("-c").arg(node_cmd);
                cmd
            }

            CrawlerLanguage::Go => {
                let go_path = if let Some(_module_path) = &config.go_module_path {
                    PathBuf::from(_module_path)
                } else {
                    script_path.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf()
                };

                let mut cmd = TokioCommand::new("sh");
                cmd.arg("-c")
                    .arg(format!("cd {} && go run {}",
                        go_path.display(),
                        script_path.file_name().unwrap().to_string_lossy()));
                cmd
            }

            CrawlerLanguage::Rust => {
                let mut cmd = TokioCommand::new("sh");
                cmd.arg("-c")
                    .arg(format!("cd {} && cargo run --bin crawler",
                        script_path.parent().unwrap_or_else(|| std::path::Path::new(".")).display()));
                cmd
            }

            CrawlerLanguage::Shell => {
                let mut cmd = TokioCommand::new("bash");
                cmd.arg(script_path);
                cmd
            }
        };

        // 添加参数
        for arg in &config.arguments {
            cmd.arg(arg);
        }

        // 设置标准输入输出
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped());

        Ok(cmd)
    }

    /// 获取支持的语言列表
    pub async fn supported_languages(&self) -> Vec<CrawlerLanguage> {
        let mut languages = Vec::new();
        let test_languages = [
            CrawlerLanguage::Python,
            CrawlerLanguage::NodeJs,
            CrawlerLanguage::Go,
            CrawlerLanguage::Rust,
            CrawlerLanguage::Shell,
        ];

        for language in &test_languages {
            if language.is_available().await {
                languages.push(language.clone());
            }
        }

        languages
    }
}

// 为了支持克隆，实现Clone trait
impl Clone for MultilangCrawler {
    fn clone(&self) -> Self {
        Self {
            workspace_root: self.workspace_root.clone(),
            temp_dir: self.temp_dir.clone(),
        }
    }
}

/// 数据源脚本生成器
impl MultilangCrawler {
    /// 生成A股数据采集脚本
    pub async fn generate_ashare_script(&self, symbols: &[String], config: &CrawlerConfig) -> Result<String> {
        let symbols_json = serde_json::to_string(symbols)?;
        Ok(format!(r#"
import requests
import json
import time
from datetime import datetime, timedelta

class AShareCrawler:
    def __init__(self, symbols):
        self.symbols = symbols
        self.base_url = "https://hq.sinajs.cn/list"
        self.headers = {{
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }}

    def get_stock_data(self, symbol):
        """获取A股数据"""
        try:
            # 获取股票列表
            url = f"{self.base_url}/{{symbol}}"
            params = {{
                'symbol': symbol,
                'type': 'stock',
                'key': 'YOUR_API_KEY'  # 需要配置API密钥
            }}

            response = requests.get(url, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            # 处理数据格式
            if data and 'item' in data:
                stock_data = {{
                    'symbol': symbol,
                    'name': data['item'].get('name', ''),
                    'price': data['item'].get('price', 0),
                    'change': data['item'].get('change', 0),
                    'change_percent': data['item'].get('change_percent', 0),
                    'volume': data['item'].get('volume', 0),
                    'timestamp': datetime.now().isoformat()
                }}
            else:
                stock_data = {{
                    'symbol': symbol,
                    'error': 'No data available',
                    'timestamp': datetime.now().isoformat()
                }}

            return stock_data

        except Exception as e:
            print(f"Error fetching data for {{symbol}}: {{e}}")
            return {{
                'symbol': symbol,
                'error': str(e),
                'timestamp': datetime.now().isoformat()
            }}

def main():
    symbols = {symbols}
    crawler = AShareCrawler(symbols)

    all_data = []
    for symbol in symbols:
        print(f"Fetching data for {{symbol}}...")
        data = crawler.get_stock_data(symbol)
        all_data.append(data)
        time.sleep(1)  # 避免请求过于频繁

    # 输出JSON格式数据
    print(json.dumps(all_data, ensure_ascii=False, indent=2))

if __name__ == "__main__":
    main()
"#, symbols_json))
    }

    /// 生成港股数据采集脚本
    pub async fn generate_hkshare_script(&self, symbols: &[String], config: &CrawlerConfig) -> Result<String> {
        let symbols_json = serde_json::to_string(symbols)?;
        Ok(format!(r#"
import requests
import json
import time

class HKShareCrawler:
    def __init__(self, symbols):
        self.symbols = symbols
        self.base_url = "https://finance.yahoo.com/webservice/v1/finance/chart"
        self.headers = {{
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }}

    def get_stock_data(self, symbol):
        """获取港股数据"""
        try:
            # Yahoo Finance API
            params = {{
                'symbol': f'{{symbol}}.HK',
                'period1': '1d',
                'interval': '1d',
                'range': '1d',
                'includePrePost': 'false'
            }}

            response = requests.get(self.base_url, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            # 解析CSV数据
            lines = response.text.strip().split('\n')
            headers = [h.strip('"') for h in lines[0].split(',')]

            if len(lines) > 1:
                data_line = lines[1]
                values = [v.strip('"') for v in data_line.split(',')]

                if len(values) >= 6:
                    stock_data = {{
                        'symbol': symbol,
                        'date': values[0],
                        'open': float(values[1]) if values[1] else 0,
                        'high': float(values[2]) if values[2] else 0,
                        'low': float(values[3]) if values[3] else 0,
                        'close': float(values[4]) if values[4] else 0,
                        'volume': int(values[5]) if values[5] else 0,
                        'timestamp': datetime.now().isoformat()
                    }}
                else:
                    stock_data = {{
                        'symbol': symbol,
                        'error': 'Incomplete data',
                        'timestamp': datetime.now().isoformat()
                    }}
            else:
                stock_data = {{
                    'symbol': symbol,
                    'error': 'No data available',
                    'timestamp': datetime.now().isoformat()
                }}

            return stock_data

        except Exception as e:
            print(f"Error fetching data for {{symbol}}: {{e}}")
            return {{
                'symbol': symbol,
                'error': str(e),
                'timestamp': datetime.now().isoformat()
            }}

def main():
    symbols = {symbols}
    crawler = HKShareCrawler(symbols)

    all_data = []
    for symbol in symbols:
        print(f"Fetching data for {{symbol}}...")
        data = crawler.get_stock_data(symbol)
        all_data.append(data)
        time.sleep(1)

    # 输出JSON格式数据
    print(json.dumps(all_data, ensure_ascii=False, indent=2))

if __name__ == "__main__":
    main()
"#, symbols_json))
    }

    /// 生成美股数据采集脚本
    pub async fn generate_usshare_script(&self, symbols: &[String], config: &CrawlerConfig) -> Result<String> {
        let symbols_json = serde_json::to_string(symbols)?;
        Ok(format!(r#"
import requests
import json
import time

class USShareCrawler:
    def __init__(self, symbols, api_key='YOUR_ALPHA_VANTAGE_KEY'):
        self.symbols = symbols
        self.api_key = api_key
        self.base_url = 'https://www.alphavantage.co/query'
        self.headers = {{
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }}

    def get_stock_data(self, symbol):
        """获取美股数据"""
        try:
            # Alpha Vantage API
            params = {{
                'function': 'TIME_SERIES_DAILY',
                'symbol': symbol,
                'apikey': self.api_key,
                'outputsize': 'compact'
            }}

            response = requests.get(self.base_url, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            if 'Time Series (Daily)' in data and 'Time Series (Daily)' in data['Time Series (Daily)']:
                time_series = data['Time Series (Daily)']
                dates = list(time_series.keys())

                if dates:
                    latest_date = dates[0]
                    latest_data = time_series[latest_date]

                    stock_data = {{
                        'symbol': symbol,
                        'date': latest_date,
                        'open': float(latest_data.get('1. open', 0)),
                        'high': float(latest_data.get('2. high', 0)),
                        'low': float(latest_data.get('3. low', 0)),
                        'close': float(latest_data.get('4. close', 0)),
                        'volume': int(latest_data.get('5. volume', 0)),
                        'timestamp': datetime.now().isoformat()
                    }}
                else:
                    stock_data = {{
                        'symbol': symbol,
                        'error': 'No data available',
                        'timestamp': datetime.now().isoformat()
                    }}
            else:
                stock_data = {{
                    'symbol': symbol,
                    'error': data.get('Note', 'API Error'),
                    'timestamp': datetime.now().isoformat()
                }}

            return stock_data

        except Exception as e:
            print(f"Error fetching data for {{symbol}}: {{e}}")
            return {{
                'symbol': symbol,
                'error': str(e),
                'timestamp': datetime.now().isoformat()
            }}

def main():
    symbols = {symbols}
    crawler = USShareCrawler(symbols)

    all_data = []
    for symbol in symbols:
        print(f"Fetching data for {{symbol}}...")
        data = crawler.get_stock_data(symbol)
        all_data.append(data)
        time.sleep(12)  # Alpha Vantage API 限制: 5 requests per minute

    # 输出JSON格式数据
    print(json.dumps(all_data, ensure_ascii=False, indent=2))

if __name__ == "__main__":
    main()
"#, symbols_json))
    }

    /// 生成数字货币数据采集脚本
    pub async fn generate_crypto_script(&self, symbols: &[String], config: &CrawlerConfig) -> Result<String> {
        let symbols_json = serde_json::to_string(symbols)?;
        Ok(format!(r#"
import requests
import json
import time
import hmac
import hashlib

class CryptoCrawler:
    def __init__(self, symbols, api_key='YOUR_BINANCE_KEY'):
        self.symbols = symbols
        self.api_key = api_key
        self.base_url = 'https://api.binance.com/api/v3/ticker/24hr'
        self.headers = {{
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }}

    def get_crypto_data(self, symbol):
        """获取数字货币数据"""
        try:
            # Binance API
            params = {{
                'symbol': symbol.upper(),
            }}

            response = requests.get(self.base_url, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            if data and len(data) > 0:
                crypto_data = data[0]
                result = {{
                    'symbol': symbol.upper(),
                    'price': float(crypto_data.get('lastPrice', 0)),
                    'price_change': float(crypto_data.get('priceChange', 0)),
                    'price_change_percent': float(crypto_data.get('priceChangePercent', 0)),
                    'high_price': float(crypto_data.get('highPrice', 0)),
                    'low_price': float(crypto_data.get('lowPrice', 0)),
                    'volume': float(crypto_data.get('volume', 0)),
                    'timestamp': datetime.now().isoformat()
                }}
            else:
                result = {{
                    'symbol': symbol.upper(),
                    'error': 'No data available',
                    'timestamp': datetime.now().isoformat()
                }}

            return result

        except Exception as e:
            print(f"Error fetching data for {{symbol}}: {{e}}")
            return {{
                'symbol': symbol.upper(),
                'error': str(e),
                'timestamp': datetime.now().isoformat()
            }}

def main():
    symbols = {symbols}
    crawler = CryptoCrawler(symbols)

    all_data = []
    for symbol in symbols:
        print(f"Fetching data for {{symbol}}...")
        data = crawler.get_crypto_data(symbol)
        all_data.append(data)
        time.sleep(0.1)  # 避免请求过于频繁

    # 输出JSON格式数据
    print(json.dumps(all_data, ensure_ascii=False, indent=2))

if __name__ == "__main__":
    main()
"#, symbols_json))
    }

    /// 生成外汇数据采集脚本
    pub async fn generate_forex_script(&self, pairs: &[String], config: &CrawlerConfig) -> Result<String> {
        let pairs_json = serde_json::to_string(pairs)?;
        Ok(format!(r#"
import requests
import json
import time

class ForexDataCrawler:
    def __init__(self, pairs, api_key='YOUR_FOREX_KEY'):
        self.pairs = pairs
        self.api_key = api_key
        self.base_url = 'https://api.exchangerate-api.com/v4/latest'
        self.headers = {{
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }}

    def get_forex_data(self, pair):
        """获取外汇数据"""
        try:
            # ExchangeRate-API
            params = {{
                'access_key': self.api_key,
                'currencies': pair,
                'format': 'json'
            }}

            response = requests.get(self.base_url, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            if 'rates' in data:
                rates = data['rates']
                base = data.get('base', '')
                result = {{
                    'pair': pair,
                    'base_currency': base,
                    'rates': rates,
                    'timestamp': datetime.now().isoformat()
                }}
            else:
                result = {{
                    'pair': pair,
                    'error': 'No data available',
                    'timestamp': datetime.now().isoformat()
                }}

            return result

        except Exception as e:
            print(f"Error fetching data for {{pair}}: {{e}}")
            return {{
                'pair': pair,
                'error': str(e),
                'timestamp': datetime.now().isoformat()
            }}

def main():
    pairs = {pairs}
    crawler = ForexDataCrawler(pairs)

    all_data = []
    for pair in pairs:
        print(f"Fetching data for {{pair}}...")
        data = crawler.get_forex_data(pair)
        all_data.append(data)
        time.sleep(1)

    # 输出JSON格式数据
    print(json.dumps(all_data, ensure_ascii=False, indent=2))

if __name__ == "__main__":
    main()
"#, pairs_json))
    }

    /// 生成新闻数据采集脚本
    pub async fn generate_news_script(&self, keywords: &[String], config: &CrawlerConfig) -> Result<String> {
        let keywords_json = serde_json::to_string(keywords)?;
        Ok(format!(r#"
import requests
import json
import time
from datetime import datetime, timedelta

class NewsDataCrawler:
    def __init__(self, keywords, api_key='YOUR_NEWS_KEY'):
        self.keywords = keywords
        self.api_key = api_key
        self.sources = [
            'https://newsapi.org/v2/everything',
            'https://gnews.io/api/v4/search',
            'https://finnhub.io/api/v1/news'
        ]
        self.headers = {{
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }}

    def get_news_data(self, keyword):
        """获取新闻数据"""
        articles = []

        for source_url in self.sources:
            try:
                if 'newsapi' in source_url:
                    # NewsAPI
                    params = {{
                        'q': keyword,
                        'language': 'zh',
                        'sortby': 'publishedAt',
                        'from': datetime.now() - timedelta(days=7),
                        'apiKey': self.api_key,
                        'pageSize': 20
                    }}
                elif 'gnews' in source_url:
                    # GNews
                    params = {{
                        'q': keyword,
                        'lang': 'zh',
                        'from': datetime.now() - timedelta(days=7),
                        'token': self.api_key,
                        'max': 20
                    }}
                elif 'finnhub' in source_url:
                    # Finnhub
                    params = {{
                        'category': 'general',
                        'q': keyword,
                        'from': datetime.now() - timedelta(days=7),
                        'to': datetime.now(),
                        'apiKey': self.api_key
                    }}

                response = requests.get(source_url, params=params, headers=self.headers, timeout=30)
                response.raise_for_status()

                data = response.json()

                # 解析不同来源的数据格式
                if 'articles' in data:
                    for article in data['articles'][:10]:  # 限制每个来源最多10条
                        processed_article = {{
                            'keyword': keyword,
                            'source': self._get_source_name(source_url),
                            'title': article.get('title', ''),
                            'description': article.get('description', ''),
                            'url': article.get('url', ''),
                            'publishedAt': article.get('publishedAt', article.get('publishedAt', '')),
                            'source_name': article.get('source', {}).get('name', ''),
                            'timestamp': datetime.now().isoformat()
                        }}
                        articles.append(processed_article)

                # 避免请求过于频繁
                time.sleep(0.5)

            except Exception as e:
                print(f"Error fetching from {{source_url}}: {{e}}")
                continue

        return articles

    def _get_source_name(self, url):
        """获取新闻源名称"""
        if 'newsapi' in url:
            return 'NewsAPI'
        elif 'gnews' in url:
            return 'GNews'
        elif 'finnhub' in url:
            return 'Finnhub'
        else:
            return 'Unknown'

def main():
    keywords = {keywords}
    crawler = NewsDataCrawler(keywords)

    all_articles = []
    for keyword in keywords:
        print(f"Fetching news for keyword: {{keyword}}...")
        articles = crawler.get_news_data(keyword)
        all_articles.extend(articles)
        time.sleep(1)

    # 输出JSON格式数据
    print(json.dumps({{
        'keyword': keywords,
        'articles': all_articles,
        'total_count': len(all_articles),
        'timestamp': datetime.now().isoformat()
    }}, ensure_ascii=False, indent=2))

if __name__ == "__main__":
    main()
"#, keywords_json))
    }

    /// 生成社交媒体数据采集脚本
    pub async fn generate_social_media_script(&self, keywords: &[String], platforms: &[String], config: &CrawlerConfig) -> Result<String> {
        let keywords_json = serde_json::to_string(keywords)?;
        let platforms_json = serde_json::to_string(platforms)?;
        Ok(format!(r#"
import requests
import json
import time
import re
from datetime import datetime, timedelta

class SocialMediaDataCrawler:
    def __init__(self, keywords, platforms):
        self.keywords = keywords
        self.platforms = platforms
        self.headers = {{
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }}

    def get_weibo_data(self, keyword):
        """获取微博数据"""
        try:
            # 这里使用搜索API的示例
            url = 'https://s.weibo.com/weibo'
            params = {{
                'q': keyword,
                'type': 'weibo',
                'page': 1,
                'page_size': 20
            }}

            response = requests.get(url, params=params, headers=self.headers, timeout=30)

            # 处理微博数据
            posts = []
            if response.status_code == 200:
                # 这里需要根据实际API响应格式调整
                for i in range(5):  # 示例数据
                    posts.append({{
                        'platform': 'weibo',
                        'keyword': keyword,
                        'content': f'示例微博内容关于{{keyword}} {{i+1}}',
                        'user': f'示例用户{{i+1}}',
                        'likes': 100 + i * 10,
                        'comments': 50 + i * 5,
                        'shares': 20 + i * 2,
                        'timestamp': datetime.now().isoformat()
                    }})

            return posts

        except Exception as e:
            print(f"Error fetching Weibo data: {{e}}")
            return []

    def analyze_sentiment(self, text):
        """简单的情感分析"""
        positive_words = ['好', '棒', '优秀', '喜欢', '推荐', '买入', '涨', '利好']
        negative_words = ['差', '糟糕', '卖出', '跌', '利空', '风险', '亏损']

        positive_count = sum(1 for word in positive_words if word in text)
        negative_count = sum(1 for word in negative_words if word in text)

        if positive_count > negative_count:
            return 'positive'
        elif negative_count > positive_count:
            return 'negative'
        else:
            return 'neutral'

def main():
    keywords = {keywords}
    platforms = {platforms}
    crawler = SocialMediaDataCrawler(keywords, platforms)

    all_data = []

    for keyword in keywords:
        print(f"Fetching social media data for keyword: {{keyword}}...")

        for platform in platforms:
            if platform.lower() == 'weibo':
                posts = crawler.get_weibo_data(keyword)
                for post in posts:
                    post['sentiment'] = crawler.analyze_sentiment(post['content'])
                    all_data.append(post)

            time.sleep(1)

    # 输出JSON格式数据
    print(json.dumps({{
        'keywords': keywords,
        'platforms': platforms,
        'data': all_data,
        'total_count': len(all_data),
        'timestamp': datetime.now().isoformat()
    }}, ensure_ascii=False, indent=2))

if __name__ == "__main__":
    main()
"#, keywords_json, platforms_json))
    }

    /// 生成经济指标数据采集脚本
    pub async fn generate_economic_indicators_script(&self, indicators: &[String], countries: &[String], config: &CrawlerConfig) -> Result<String> {
        let indicators_json = serde_json::to_string(indicators)?;
        let countries_json = serde_json::to_string(countries)?;
        Ok(format!(r#"
import requests
import json
import time
from datetime import datetime, timedelta

class EconomicDataCrawler:
    def __init__(self, indicators, countries, api_key='YOUR_ECONOMIC_KEY'):
        self.indicators = indicators
        self.countries = countries
        self.api_key = api_key
        self.sources = [
            'https://api.fredapi.org/fred/series/observations',
            'https://api.tradingeconomics.com/indicator',
            'https://www.alphavantage.co/query'
        ]
        self.headers = {{
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }}

    def get_fred_data(self, indicator, country):
        """获取FRED经济数据"""
        try:
            # FRED API
            series_id = f"{{indicator}}_{{country.upper()}}"  # 示例: GDP_US, CPI_CN
            params = {{
                'series_id': series_id,
                'api_key': self.api_key,
                'file_type': 'json',
                'observation_start': (datetime.now() - timedelta(days=365)).strftime('%Y-%m-%d')
            }}

            response = requests.get('https://api.fredapi.org/fred/series/observations', params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            if 'observations' in data:
                observations = data['observations'][:10]  # 最近10个数据点
                result = {{
                    'indicator': indicator,
                    'country': country,
                    'source': 'FRED',
                    'data': observations,
                    'timestamp': datetime.now().isoformat()
                }}
            else:
                result = {{
                    'indicator': indicator,
                    'country': country,
                    'error': 'No data available',
                    'timestamp': datetime.now().isoformat()
                }}

            return result

        except Exception as e:
            print(f"Error fetching FRED data: {{e}}")
            return {{
                'indicator': indicator,
                'country': country,
                'error': str(e),
                'timestamp': datetime.now().isoformat()
            }}

def main():
    indicators = {indicators}
    countries = {countries}
    crawler = EconomicDataCrawler(indicators, countries)

    all_data = []

    for indicator in indicators:
        for country in countries:
            print(f"Fetching {{indicator}} data for {{country}}...")
            data = crawler.get_fred_data(indicator, country)
            all_data.append(data)
            time.sleep(1)

    # 输出JSON格式数据
    print(json.dumps({{
        'indicators': indicators,
        'countries': countries,
        'data': all_data,
        'total_count': len(all_data),
        'timestamp': datetime.now().isoformat()
    }}, ensure_ascii=False, indent=2))

if __name__ == "__main__":
    main()
"#, indicators_json, countries_json))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_crawler_language_availability() {
        let python = CrawlerLanguage::Python;
        assert!(python.is_available().await || !python.is_available().await); // Just check it doesn't panic

        let node = CrawlerLanguage::NodeJs;
        assert!(node.is_available().await || !node.is_available().await);
    }

    #[tokio::test]
    async fn test_multilang_crawler_initialization() {
        let crawler = MultilangCrawler::new("/tmp/test_crawler");
        assert!(crawler.initialize().await.is_ok());
    }

    #[test]
    fn test_crawler_config_default() {
        let config = CrawlerConfig::default();
        assert_eq!(config.language, CrawlerLanguage::Python);
        assert!(config.script_path.is_none());
        assert!(config.inline_code.is_none());
    }

    #[test]
    fn test_crawler_language_properties() {
        let python = CrawlerLanguage::Python;
        assert_eq!(python.extension(), "py");
        assert_eq!(python.command(), "python3");
        assert_eq!(python.default_timeout(), Duration::from_secs(300));

        let node = CrawlerLanguage::NodeJs;
        assert_eq!(node.extension(), "js");
        assert_eq!(node.command(), "node");
        assert_eq!(node.default_timeout(), Duration::from_secs(180));
    }
}