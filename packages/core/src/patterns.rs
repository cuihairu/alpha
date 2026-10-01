//! 形态识别（L500 前半：技术分析「形态」面；波浪理论在
//! [`crate::indicators::advanced`]）。
//!
//! 两层识别面，全部纯函数、阈值显式参数化（无隐藏魔法数）：
//! * **K 线形态**：单根（十字星/锤子/射击之星）、双根（吞没）、
//!   三根（晨星/暮星）——纯几何特征，**不含趋势上下文判断**
//!   （锤子在下跌末端才是反转信号，上下文过滤是调用方的策略层职责）；
//! * **线形形态**：双顶/双底、头肩顶/头肩底——全部从 [`find_swings`]
//!   的 zigzag 摆动序列上识别，摆动幅度阈值过滤掉噪声级波动。
//!
//! 口径约定：
//! * 摆动枢轴是「确认制」：极值必须被反向 ≥ threshold 的波动确认才
//!   入列，尾部未确认极值不 emit——识别结果天然滞后于行情，这是
//!   确认制换低误报的代价，不存在前瞻；
//! * 多根 K 线形态锚定在**最后一根**的下标上（扫描时即信号出现位）。

use serde::{Deserialize, Serialize};

/// OHLC 蜡烛。字段关系由调用方保证（`high >= max(open, close)`、
/// `low <= min(open, close)`），[`Bar::new`] 在 debug 构建下断言。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

impl Bar {
    pub fn new(open: f64, high: f64, low: f64, close: f64) -> Self {
        debug_assert!(
            high >= open && high >= close && low <= open && low <= close,
            "OHLC 关系非法: O{open} H{high} L{low} C{close}"
        );
        Self {
            open,
            high,
            low,
            close,
        }
    }

    /// 实体绝对值 |close - open|
    pub fn body(&self) -> f64 {
        (self.close - self.open).abs()
    }

    /// 全幅 high - low
    pub fn range(&self) -> f64 {
        self.high - self.low
    }

    /// 上影线
    pub fn upper_shadow(&self) -> f64 {
        self.high - self.open.max(self.close)
    }

    /// 下影线
    pub fn lower_shadow(&self) -> f64 {
        self.open.min(self.close) - self.low
    }

    pub fn is_bullish(&self) -> bool {
        self.close > self.open
    }

    pub fn is_bearish(&self) -> bool {
        self.close < self.open
    }
}

/// 十字星：实体 ≤ `max_body_ratio × 全幅`（全幅为 0 的平线不算）
pub fn is_doji(candle: Bar, max_body_ratio: f64) -> bool {
    let range = candle.range();
    range > 0.0 && candle.body() <= max_body_ratio * range
}

/// 锤子（几何特征）：下影 ≥ 2×实体、上影 ≤ 实体——长下影代表下方有承接
pub fn is_hammer(candle: Bar) -> bool {
    let body = candle.body();
    candle.range() > 0.0
        && body > 0.0
        && candle.lower_shadow() >= 2.0 * body
        && candle.upper_shadow() <= body
}

/// 射击之星（锤子的镜像）：上影 ≥ 2×实体、下影 ≤ 实体
pub fn is_shooting_star(candle: Bar) -> bool {
    let body = candle.body();
    candle.range() > 0.0
        && body > 0.0
        && candle.upper_shadow() >= 2.0 * body
        && candle.lower_shadow() <= body
}

/// 看涨吞没：前阴后阳，阳线实体完全包住阴线实体且更大
pub fn is_bullish_engulfing(prev: Bar, cur: Bar) -> bool {
    prev.is_bearish()
        && cur.is_bullish()
        && cur.open <= prev.close
        && cur.close >= prev.open
        && cur.body() > prev.body()
}

/// 看跌吞没：前阳后阴，阴线实体完全包住阳线实体且更大
pub fn is_bearish_engulfing(prev: Bar, cur: Bar) -> bool {
    prev.is_bullish()
        && cur.is_bearish()
        && cur.open >= prev.close
        && cur.close <= prev.open
        && cur.body() > prev.body()
}

/// 晨星（三根，看涨）：大阴 → 小实体星线（实体 ≤ ½ 阴线实体）→
/// 阳线收在阴线实体中点之上
pub fn is_morning_star(a: Bar, b: Bar, c: Bar) -> bool {
    a.is_bearish()
        && b.body() <= 0.5 * a.body()
        && c.is_bullish()
        && c.close > (a.open + a.close) / 2.0
}

/// 暮星（三根，看跌）：大阳 → 小实体星线 → 阴线收在阳线实体中点之下
pub fn is_evening_star(a: Bar, b: Bar, c: Bar) -> bool {
    a.is_bullish()
        && b.body() <= 0.5 * a.body()
        && c.is_bearish()
        && c.close < (a.open + a.close) / 2.0
}

/// K 线形态种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandlePattern {
    Doji,
    Hammer,
    ShootingStar,
    BullishEngulfing,
    BearishEngulfing,
    MorningStar,
    EveningStar,
}

/// 形态方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Bias {
    Bullish,
    Bearish,
    Neutral,
}

/// 一次形态命中：`index` 为形态锚定 bar（多根形态取最后一根）的下标
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PatternHit {
    pub index: usize,
    pub pattern: CandlePattern,
    pub bias: Bias,
}

/// 扫描 K 线序列，按时间序返回全部命中（同一根可命中多个形态）。
/// `doji_max_body_ratio` 为十字星实体占比上限（常用 0.1）。
pub fn detect_candle_patterns(bars: &[Bar], doji_max_body_ratio: f64) -> Vec<PatternHit> {
    let mut hits = Vec::new();
    for (i, &candle) in bars.iter().enumerate() {
        if is_doji(candle, doji_max_body_ratio) {
            hits.push(PatternHit {
                index: i,
                pattern: CandlePattern::Doji,
                bias: Bias::Neutral,
            });
        }
        if is_hammer(candle) {
            hits.push(PatternHit {
                index: i,
                pattern: CandlePattern::Hammer,
                bias: Bias::Bullish,
            });
        }
        if is_shooting_star(candle) {
            hits.push(PatternHit {
                index: i,
                pattern: CandlePattern::ShootingStar,
                bias: Bias::Bearish,
            });
        }
        if i >= 1 {
            let prev = bars[i - 1];
            if is_bullish_engulfing(prev, candle) {
                hits.push(PatternHit {
                    index: i,
                    pattern: CandlePattern::BullishEngulfing,
                    bias: Bias::Bullish,
                });
            }
            if is_bearish_engulfing(prev, candle) {
                hits.push(PatternHit {
                    index: i,
                    pattern: CandlePattern::BearishEngulfing,
                    bias: Bias::Bearish,
                });
            }
        }
        if i >= 2 {
            let (a, b) = (bars[i - 2], bars[i - 1]);
            if is_morning_star(a, b, candle) {
                hits.push(PatternHit {
                    index: i,
                    pattern: CandlePattern::MorningStar,
                    bias: Bias::Bullish,
                });
            }
            if is_evening_star(a, b, candle) {
                hits.push(PatternHit {
                    index: i,
                    pattern: CandlePattern::EveningStar,
                    bias: Bias::Bearish,
                });
            }
        }
    }
    hits
}

// ---------- 线形形态（zigzag 摆动识别） ----------

/// 摆动枢轴种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwingKind {
    High,
    Low,
}

/// 摆动枢轴：价格序列下标 + 极值价 + 种类
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Swing {
    pub index: usize,
    pub price: f64,
    pub kind: SwingKind,
}

/// zigzag 摆动检测：从价格序列提取「确认制」交替枢轴。
///
/// 极值必须被反向 ≥ `threshold`（价格比例，如 0.02 = 2%）的波动确认
/// 才入列；尾部未确认极值不 emit。结果天然滞后于行情（确认制换低
/// 误报），无前瞻。方向未定阶段同时盯运行最高/最低，先走出反转的
/// 一侧确认对侧极值为首个枢轴（首段上涨 → 起点入列 Low）。
/// 价格需为正（threshold 以价格比例计量）。
pub fn find_swings(prices: &[f64], threshold: f64) -> Vec<Swing> {
    let mut swings = Vec::new();
    if prices.len() < 2 || !(0.0..1.0).contains(&threshold) || prices[0] <= 0.0 {
        return swings;
    }

    // 方向未定：运行最高/最低双端盯梢
    let (mut min_i, mut min_p) = (0usize, prices[0]);
    let (mut max_i, mut max_p) = (0usize, prices[0]);
    let mut dir_up: Option<bool> = None;
    let (mut ext_i, mut ext_p) = (0usize, prices[0]);

    for (i, &p) in prices.iter().enumerate().skip(1) {
        let reversed = match dir_up {
            None => {
                if (p - min_p) >= min_p * threshold {
                    swings.push(Swing {
                        index: min_i,
                        price: min_p,
                        kind: SwingKind::Low,
                    });
                    dir_up = Some(true);
                    ext_i = i;
                    ext_p = p;
                } else if (max_p - p) >= max_p * threshold {
                    swings.push(Swing {
                        index: max_i,
                        price: max_p,
                        kind: SwingKind::High,
                    });
                    dir_up = Some(false);
                    ext_i = i;
                    ext_p = p;
                } else {
                    if p < min_p {
                        min_i = i;
                        min_p = p;
                    }
                    if p > max_p {
                        max_i = i;
                        max_p = p;
                    }
                }
                false
            }
            Some(up) => {
                let extends = if up { p >= ext_p } else { p <= ext_p };
                if extends {
                    ext_i = i;
                    ext_p = p;
                    false
                } else if up {
                    (ext_p - p) >= ext_p * threshold
                } else {
                    (p - ext_p) >= ext_p * threshold
                }
            }
        };

        if reversed {
            // 上升段的极值是 High，下降段的极值是 Low
            let kind = if dir_up == Some(true) {
                SwingKind::High
            } else {
                SwingKind::Low
            };
            swings.push(Swing {
                index: ext_i,
                price: ext_p,
                kind,
            });
            dir_up = Some(!dir_up.unwrap());
            ext_i = i;
            ext_p = p;
        }
    }
    swings
}

/// 线形形态种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChartPatternKind {
    DoubleTop,
    DoubleBottom,
    HeadShoulders,
    InverseHeadShoulders,
}

/// 一次线形形态命中：`start_index` 首枢轴、`confirm_index` 确认枢轴
/// （价格序列下标）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChartPattern {
    pub kind: ChartPatternKind,
    pub bias: Bias,
    pub start_index: usize,
    pub confirm_index: usize,
}

/// 双顶：相邻两个摆动高点价差 ≤ `height_tol`（相对较高者），中间谷底
/// 深度 ≥ `min_valley_depth`（相对峰的回撤）
pub fn detect_double_top(
    prices: &[f64],
    swing_threshold: f64,
    height_tol: f64,
    min_valley_depth: f64,
) -> Vec<ChartPattern> {
    let swings = find_swings(prices, swing_threshold);
    let mut out = Vec::new();
    for w in swings.windows(3) {
        let (h1, v, h2) = (&w[0], &w[1], &w[2]);
        if h1.kind != SwingKind::High || h2.kind != SwingKind::High {
            continue;
        }
        let higher = h1.price.max(h2.price);
        if (h1.price - h2.price).abs() / higher > height_tol {
            continue;
        }
        let lower_peak = h1.price.min(h2.price);
        if (lower_peak - v.price) / lower_peak < min_valley_depth {
            continue;
        }
        out.push(ChartPattern {
            kind: ChartPatternKind::DoubleTop,
            bias: Bias::Bearish,
            start_index: h1.index,
            confirm_index: h2.index,
        });
    }
    out
}

/// 双底：双顶的镜像（看涨）
pub fn detect_double_bottom(
    prices: &[f64],
    swing_threshold: f64,
    height_tol: f64,
    min_valley_depth: f64,
) -> Vec<ChartPattern> {
    let swings = find_swings(prices, swing_threshold);
    let mut out = Vec::new();
    for w in swings.windows(3) {
        let (b1, v, b2) = (&w[0], &w[1], &w[2]);
        if b1.kind != SwingKind::Low || b2.kind != SwingKind::Low {
            continue;
        }
        let lower = b1.price.min(b2.price);
        if (b1.price - b2.price).abs() / lower > height_tol {
            continue;
        }
        // 中峰（两底之间的反弹高点）须高于两底至少 min_valley_depth
        let higher_bottom = b1.price.max(b2.price);
        if (v.price - higher_bottom) / higher_bottom < min_valley_depth {
            continue;
        }
        out.push(ChartPattern {
            kind: ChartPatternKind::DoubleBottom,
            bias: Bias::Bullish,
            start_index: b1.index,
            confirm_index: b2.index,
        });
    }
    out
}

/// 头肩顶：H–L–H–L–H 五摆动，头为最高点，两肩价差 ≤ `height_tol`
/// （相对头），两谷（颈线端点）价差 ≤ `neck_tol`（相对头）
pub fn detect_head_and_shoulders(
    prices: &[f64],
    swing_threshold: f64,
    height_tol: f64,
    neck_tol: f64,
) -> Vec<ChartPattern> {
    let swings = find_swings(prices, swing_threshold);
    let mut out = Vec::new();
    for w in swings.windows(5) {
        let (ls, v1, head, v2, rs) = (&w[0], &w[1], &w[2], &w[3], &w[4]);
        if ls.kind != SwingKind::High || head.kind != SwingKind::High || rs.kind != SwingKind::High
        {
            continue;
        }
        // 头严格高于两肩；肩差与颈线端点差都相对头计量
        if head.price <= ls.price || head.price <= rs.price {
            continue;
        }
        if (ls.price - rs.price).abs() / head.price > height_tol {
            continue;
        }
        if (v1.price - v2.price).abs() / head.price > neck_tol {
            continue;
        }
        out.push(ChartPattern {
            kind: ChartPatternKind::HeadShoulders,
            bias: Bias::Bearish,
            start_index: ls.index,
            confirm_index: rs.index,
        });
    }
    out
}

/// 头肩底：头肩顶的镜像（看涨）
pub fn detect_inverse_head_and_shoulders(
    prices: &[f64],
    swing_threshold: f64,
    height_tol: f64,
    neck_tol: f64,
) -> Vec<ChartPattern> {
    let swings = find_swings(prices, swing_threshold);
    let mut out = Vec::new();
    for w in swings.windows(5) {
        let (ls, v1, head, v2, rs) = (&w[0], &w[1], &w[2], &w[3], &w[4]);
        if ls.kind != SwingKind::Low || head.kind != SwingKind::Low || rs.kind != SwingKind::Low {
            continue;
        }
        if head.price >= ls.price || head.price >= rs.price {
            continue;
        }
        if (ls.price - rs.price).abs() / ls.price.max(rs.price) > height_tol {
            continue;
        }
        if (v1.price - v2.price).abs() / ls.price.max(rs.price) > neck_tol {
            continue;
        }
        out.push(ChartPattern {
            kind: ChartPatternKind::InverseHeadShoulders,
            bias: Bias::Bullish,
            start_index: ls.index,
            confirm_index: rs.index,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_bar_geometry() {
        // 十字星：实体 0.1 / 全幅 2.0 = 5% ≤ 10%
        let doji = Bar::new(10.0, 11.0, 9.0, 10.1);
        assert!(is_doji(doji, 0.1));
        assert!(!is_doji(doji, 0.04));
        // 平线不算十字星
        assert!(!is_doji(Bar::new(10.0, 10.0, 10.0, 10.0), 0.1));

        // 锤子：实体 1、下影 3 ≥ 2×1、上影 0.5 ≤ 1
        let hammer = Bar::new(10.0, 10.5, 6.5, 9.0);
        assert!(is_hammer(hammer));
        assert!(!is_shooting_star(hammer));
        // 上影过长破坏锤子
        assert!(!is_hammer(Bar::new(10.0, 13.0, 6.5, 9.0)));

        // 射击之星：镜像
        let star = Bar::new(10.0, 13.5, 9.5, 11.0);
        assert!(is_shooting_star(star));
        assert!(!is_hammer(star));
        // 无实体 bar 两形态都不成立（要求 body > 0）
        assert!(!is_hammer(Bar::new(10.0, 12.0, 8.0, 10.0)));
    }

    #[test]
    fn multi_bar_candle_patterns() {
        let bear = Bar::new(10.0, 10.2, 8.0, 8.5); // 大阴：实体 1.5
        let small = Bar::new(8.6, 9.0, 8.3, 8.8); // 星线：实体 0.2 ≤ ½×1.5
        let bull = Bar::new(8.8, 10.5, 8.7, 10.0); // 阳线收 10 > 阴线中点 9.25
        assert!(is_morning_star(bear, small, bull));

        // 阳线收不进中点 → 不成立
        assert!(!is_morning_star(bear, small, Bar::new(8.8, 10.5, 8.7, 9.0)));

        let big_bull = Bar::new(8.0, 12.0, 7.8, 11.5);
        let star = Bar::new(11.4, 11.8, 11.0, 11.2);
        let big_bear = Bar::new(11.2, 11.4, 8.0, 8.5);
        assert!(is_evening_star(big_bull, star, big_bear));

        // 吞没：前阴 [9.5→9.0]，后阳 [8.9→10.0] 包住且更大
        let prev = Bar::new(9.5, 9.6, 8.9, 9.0);
        let cur = Bar::new(8.9, 10.1, 8.8, 10.0);
        assert!(is_bullish_engulfing(prev, cur));
        assert!(!is_bearish_engulfing(prev, cur));
        // 实体没包住：开盘价高于前收
        assert!(!is_bullish_engulfing(prev, Bar::new(9.2, 10.1, 9.1, 10.0)));
        // 反向吞没
        let prev_bull = Bar::new(9.0, 10.1, 8.9, 10.0);
        let cur_bear = Bar::new(10.1, 10.2, 8.8, 9.0);
        assert!(is_bearish_engulfing(prev_bull, cur_bear));
    }

    #[test]
    fn candle_scanner_anchors_hits() {
        // bars[0] 锤子，bars[1..3] 构成晨星（锚定 index 2）
        let bars = vec![
            Bar::new(10.0, 10.5, 6.5, 9.0),
            Bar::new(9.0, 9.2, 7.0, 7.5),
            Bar::new(7.3, 7.6, 7.1, 7.4),
            Bar::new(7.4, 9.5, 7.3, 9.0),
        ];
        let hits = detect_candle_patterns(&bars, 0.1);
        assert!(hits.contains(&PatternHit {
            index: 0,
            pattern: CandlePattern::Hammer,
            bias: Bias::Bullish
        }));
        assert!(
            hits.iter()
                .any(|h| h.index == 3 && h.pattern == CandlePattern::MorningStar),
            "晨星锚定在第三根: {hits:?}"
        );
    }

    /// zigzag 手算：[10,12,10.5,13,11]、阈值 5% → L(0,10) H(1,12)
    /// L(2,10.5) H(3,13)，尾部未确认极值 (4,11) 不 emit
    #[test]
    fn swings_are_confirmed_alternating() {
        let prices = [10.0, 12.0, 10.5, 13.0, 11.0];
        let swings = find_swings(&prices, 0.05);
        assert_eq!(
            swings,
            vec![
                Swing {
                    index: 0,
                    price: 10.0,
                    kind: SwingKind::Low
                },
                Swing {
                    index: 1,
                    price: 12.0,
                    kind: SwingKind::High
                },
                Swing {
                    index: 2,
                    price: 10.5,
                    kind: SwingKind::Low
                },
                Swing {
                    index: 3,
                    price: 13.0,
                    kind: SwingKind::High
                },
            ]
        );
        // 阈值吃掉全部波动 → 无枢轴
        assert!(find_swings(&prices, 0.9).is_empty());
        assert!(find_swings(&prices[..1], 0.05).is_empty());
    }

    /// 首段上涨的序列：起点 Low 也要入列（上升被回落确认时 emit）
    #[test]
    fn leading_low_emitted_on_first_up_reversal() {
        let prices = [10.0, 12.0, 11.0]; // +20% 后回落 8.3%
        let swings = find_swings(&prices, 0.05);
        assert_eq!(
            swings,
            vec![
                Swing {
                    index: 0,
                    price: 10.0,
                    kind: SwingKind::Low
                },
                Swing {
                    index: 1,
                    price: 12.0,
                    kind: SwingKind::High
                },
            ]
        );
    }

    #[test]
    fn double_top_and_bottom() {
        // 双顶：峰 15/15，谷 11（深度 26.7% ≥ 15%）
        let top = [10.0, 15.0, 11.0, 15.0, 12.0];
        let hits = detect_double_top(&top, 0.03, 0.05, 0.15);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].start_index, 1);
        assert_eq!(hits[0].confirm_index, 3);
        assert_eq!(hits[0].bias, Bias::Bearish);

        // 峰差超容差 → 不识别
        assert!(detect_double_bottom(&top, 0.03, 0.05, 0.15).is_empty());

        // 双底：底 8/8.1（差 1.2% ≤ 5%），中间峰 10（深度 ≥ 15%）
        let bottom = [12.0, 8.0, 10.0, 8.1, 9.5];
        let hits = detect_double_bottom(&bottom, 0.03, 0.05, 0.15);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].bias, Bias::Bullish);
        // 谷深不足（8.0→9.0 峰谷差 10% < 15%）→ 不识别
        let shallow = [12.0, 8.0, 9.0, 8.1, 9.5];
        assert!(detect_double_bottom(&shallow, 0.03, 0.05, 0.15).is_empty());
    }

    #[test]
    fn head_and_shoulders_both_directions() {
        // 头肩顶：肩 12/12.2（差 1.3% ≤ 5%），头 15，颈线端点 10.8/11
        let hs = [10.0, 12.0, 10.8, 15.0, 11.0, 12.2, 10.5];
        let hits = detect_head_and_shoulders(&hs, 0.03, 0.05, 0.05);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].start_index, 1);
        assert_eq!(hits[0].confirm_index, 5);
        assert_eq!(hits[0].bias, Bias::Bearish);

        // 头不够高（头 = 左肩）→ 不识别
        let flat_head = [10.0, 15.0, 13.0, 15.0, 13.5, 14.2, 12.0];
        assert!(detect_head_and_shoulders(&flat_head, 0.03, 0.05, 0.05).is_empty());

        // 头肩底：镜像
        let ihs = [15.0, 11.0, 12.5, 9.0, 12.2, 11.0, 13.5];
        let hits = detect_inverse_head_and_shoulders(&ihs, 0.03, 0.05, 0.05);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].bias, Bias::Bullish);
    }
}
