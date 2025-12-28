# Crawlers

这个目录用于放置“可独立运行”的多语言爬虫脚本/项目（Python/Node/Go/Rust/Shell 等）。

- 推荐结构：`crawlers/<language>/<crawler_name>/...` 或 `crawlers/<language>/<crawler_name>.py`
- 约定：爬虫尽量将结果输出到 stdout（JSON），方便被 `services/collector` 的多语言执行器接入

快速示例（Python）：

- `crawlers/python/eastmoney_quote.py`

