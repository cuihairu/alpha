# Parquet 数据湖存储架构设计

口径：**设计项**（沿 L34 跨平台架构等设计项先例）——本文档定架构与物理布局，
零代码改动；落地实现归「智能数据分区策略（L447）」及后续工程项。

上游依托：packages/storage（ClickHouse `market_data` 七列事实 schema）、
services/data-engine（DataFusion 35 `MemTable` 热查询 + `/clickhouse/export.parquet`
HTTP 即时导出）、desktop/web 侧 DuckDB `read_parquet()` 既有消费习惯。

## 1. 目标与现状差距（实测）

| 现状 | 差距 |
|---|---|
| Parquet 仅按请求即时生成（HTTP 导出，内存中转） | 无持久湖层：分析侧每次重算都要回源 ClickHouse 全量拉取 |
| DataFusion 只注册 `MemTable`（单批内存表） | 无 ListingTable：不能按分区裁剪、不能扫描湖上历史 |
| ClickHouse/TimescaleDB 承担全部存储 | 冷数据与热查询混布：保留策略受查询层成本绑架 |

目标：引入 **Parquet 湖层**作为分析侧持久批量层——写一次、多引擎读
（DataFusion / DuckDB web+desktop / 未来 Ray/Spark 类）、与热查询层解耦保留。

## 2. 分层模型（湖不替代热库）

```
collector ──写──► ClickHouse/TimescaleDB（热层，N 天，点查/订阅）
     │
     └──落湖──► 数据湖（本设计）
                Bronze  原始采集回放（采集原样，含脏数据标记）
                Silver  标准 OHLCV（清洗/去重/类型化后，主分析面）
                Gold    聚合特征（指标/因子，随分析能力扩展）
```

- 湖是**分析侧**层：热查询仍走 ClickHouse/TimescaleDB，不迁移不替换。
- Silver 为首批落地层（Bronze 回放价值高但优先级低；Gold 随因子工程立项）。
- data-engine 现有 `/clickhouse/export.parquet` 端点即未来 **lake writer 接缝**：
  从「响应体直出」改为「分区文件落湖 + 返回清单」。

## 3. 物理布局（路径规范）

```
{lake_root}/{layer}/{table}/trade_date=YYYY-MM-DD/part-{seq:05d}.parquet
例：lake/silver/market_data/trade_date=2026-10-02/part-00042.parquet
```

- `trade_date` 为**交易日**（A股日历，非自然日）——分区键即目录，ListingTable
  零配置分区裁剪；时间/股票/交易所多维分区细化归 L447（本设计先定单维骨架）。
- `part-{seq}` 序号**单调递增**、由 writer 原子分配；文件内容幂等键 =
  `(layer, table, trade_date, seq)`。
- 单文件目标 128MB~512MB、行组 128MB（A股 tick 级可再调，归 L448 实测）。
- 编码：Silver 起步 **snappy**（普适兼容），zstd 级别/列字典调优归 L448；
  symbol 列启用字典编码（基数极低，压缩比收益大）。
- 文件内按 `symbol, timestamp` 排序（点查裁剪友好）。

## 4. Schema 规范（与 ClickHouse 列名零映射对齐）

| 字段 | Arrow/Parquet 类型 | 说明 |
|---|---|---|
| `timestamp` | Timestamp(ms, UTC) | 与 ClickHouse `timestamp` 对齐 |
| `symbol` | Dictionary(Int32, Utf8) | 六位代码；字典编码 |
| `open_price/high_price/low_price/close_price` | Float64 | 列名与 storage 层现 SQL 完全一致 |
| `volume` | Float64 | 列名对齐；类型按 §4 演进规则由现 UInt64 放宽为 Float64（手数口径注于表属性） |
| （Bronze 附加）`_raw` / `_ingested_at` | Utf8 / Timestamp(ms) | 原始报文与入库时间 |

- **演进规则**：列只增不删、类型只放宽不收窄；破坏性变更 → 新表名版本
  （`market_data_v2`），旧表冻结只读——不做原位迁移。
- 表属性（key-value metadata）记 `schema_version` / `trade_calendar` 溯源。

## 5. 写路径（幂等 + 原子）

1. writer 按分区聚合数据 → 写 `{partition}/.tmp/part-{seq}.parquet`；
2. `fsync` 后原子 rename 到正式名（对象存储用 put+copy 语义对应）；
3. 元数据登记（骨架期**无外部 metastore**，见 §7）。
- 重放安全：同 `(trade_date, seq)` 重写内容一致（writer 确定性），覆盖即重放。
- 小文件治理：单分区文件数阈值触发 compaction 合并（阈值与调度归 L447）。
  执行侧已落（2026-10-09，`LakeWriter::compact_table`）：扫 `{layer}/{table}`
  全分区 → 装箱计划（消费 L447 `plan_compaction`）→ 组内按文件名升序拼接
  （同 `(symbol, timestamp)` 后写者胜出，与读侧去重口径一致）→ 新 seq 落合并
  文件 → 全部组写成功后删旧文件。手动触发（写透后/定时由调用方定），无内置
  调度器（§7）。

## 6. 读路径（多引擎）

| 引擎 | 方式 | 备注 |
|---|---|---|
| data-engine（DataFusion 35） | `ListingTable` 注册 `lake_market_data`，`trade_date` 分区列 | 与现有 `MemTable` 热查询并存：热=MemTable，历史=ListingTable |
| web/desktop（DuckDB-WASM） | `read_parquet('.../trade_date=*/part-*.parquet')` 通配 | 既有 `read_parquet()` 消费习惯零改动 |
| 桌面 Tauri | 同 DuckDB（本地文件系统路径） | 接缝随 L427 §6 选项 A 落地 |

- data-engine 增量接缝（已落地 2026-10-09）：`refresh_query_tables` 每次
  `/query` 重走时注册/复用 ListingTable（`lake_market_data`）——首次落湖后
  下一查即可见；`/query` SQL 可直接 `UNION` 热/历史两层。
- 实现注记：表名用平名 `lake_market_data` 而非设计稿点分 `lake.market_data`
  （DF 点分名要建同名 schema，骨架期平名更直白）；ListingTable 走
  `ListingOptions::with_table_partition_cols([trade_date])` + `infer_schema`
  （路径即分区裁剪，零 metastore）；会话须设
  `listing_table_ignore_subdirectory=false`（DF35 默认 true，会让 `trade_date=`
  子目录下的文件列不到、表只剩分区列）。

## 7. Catalog 与元数据（骨架期取舍）

- **不引** Hive metastore/Iceberg/Delta：骨架期单写者 + 目录即清单，
  ListingTable 目录遍历即元数据；避免首个落地项背全栈表格式复杂度。
- 升级路径：分区清单文件（`_manifest.json`：分区→文件→行数/字节统计）作为
  catalog v1，挂后续项；再往后的 ACID 事务需求出现时才评估 Delta/Iceberg。
  **已落（2026-10-09）**：`LakeWriter::rebuild_manifest`/`read_manifest`——
  按需全量重建 `{layer}/{table}/_manifest.json`（行数取 Parquet footer、
  原子替换、serde 可序列化）；读侧不依赖（仍列目录），清单只作统计面与
  catalog 升级底座。

## 8. 保留与冷热分层

- ClickHouse/TimescaleDB 热：滚动 N 天（点查/订阅延迟敏感）；
- 湖 Silver：持久（默认全量保留）；归档层（对象存储冷 class）挂发布/成本项；
- 导出端点保持兼容：`/clickhouse/export.parquet` 缺省语义不变（即时导出），
  湖启用后 `?from_lake=true` 读旁路已落（data-engine，见 §9 注记）。

## 9. 与后续 TODO 的边界

| 项 | 归属 |
|---|---|
| 多维分区（时间/股票/交易所）与 compaction 调度 | **L447**（本设计 §3/§5 只定单维骨架） |
| zstd 级别/列编码/行组实测调优 | **L448**（数据压缩和列式存储优化） |
| 零拷贝读取/内存池 | 性能优化工程节 |
| lake writer 落地（改造 export 端点 + ListingTable 注册） | 本模块已落（见下）|

**lake writer 落地（2026-10-09）**：`packages/storage/src/lake.rs` 交付写路径
骨架——`LakeWriter::write_bars(layer, table, bars, seq)` 按交易日分组（复用
L447 `partition` 策略层与 `trade_date_of` 日切）→ 编 Parquet（snappy、schema
七列对齐 §4、文件内按 `symbol, timestamp` 排序）→ `.tmp` 写 + fsync + 原子
rename（§5）→ 返回清单；`read_partition` 扫描读回单分区。边界同 §10：单写者、
`seq` 由调用方分配并保证同分区单调、无 metastore（目录即清单）。

**export 端点改造落地（2026-10-09）**：data-engine 装配 `lake.*` 配置段
（默认关，关闭时零行为变化）——启用后 `/clickhouse/export.parquet?query_id=market_data`
在即时导出返回的同时把同窗口行级数据按交易日写透落湖（写失败只告警不拒绝
请求）；`?from_lake=true` 读旁路按区间枚举交易日分区读回、symbol 过滤 +
limit 截断后直出湖 schema Parquet（读侧同 `(symbol, timestamp)` 去重，不
暴露重放产生的重复行）。缺省路径与 ClickHouse 门控行为均不变。

**ListingTable 注册落地（2026-10-09）**：`refresh_query_tables` 把
`{lake_root}/{layer}/{table}` 注册为 DataFusion ListingTable
（`lake_market_data`，§6 接缝）——`trade_date` hive 目录即分区列，`/query`
SQL 直扫历史层、可与热层 MemTable `UNION`；目录未建（尚无落湖）静默跳过、
首次落湖后下一查自动注册。**未做**：compaction 内置调度器（执行侧
`compact_table` 已落，触发时机由调用方定）、对象存储适配（§10.1）——
保持登记。

## 10. 非交互假设（自行判定，已注明）

1. `lake_root` 骨架期 = 本地/单机文件系统目录（配置槽）；对象存储（S3/OSS）
   适配随发布节，路径规范两态一致（`trade_date=` hive 式目录）。
2. 单写者假设：collector 落湖与 export 落湖不同时启用（骨架期先 export 侧）。
3. 首表 = `silver/market_data`（七列对齐 §4）；realtime_quotes 等表随落地项。
4. 本单零代码：§6 接缝、§7 manifest、§5 compaction 均登记不实现。
5. 写路径骨架已落（2026-10-09，`packages/storage/src/lake.rs`）：§3 布局 / §4
   schema / §5 原子写落地；export 端点写透 + `from_lake` 读旁路、§6
   ListingTable 注册（`lake_market_data`）、§5 compaction 执行侧
   （`compact_table`，无内置调度器）、§7 manifest（`rebuild_manifest`，
   按需重建）已落（同日，data-engine `lake.*` 默认关）；对象存储适配
   （§10.1，随发布节）仍登记不实现。
