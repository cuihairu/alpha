// Model types matching the Rust FFI contract (serde JSON schema)
// These mirror the MarketData/AnalysisResult definitions in packages/core

import Foundation

// Market data snapshot – used by the Rust core demo source
public struct MarketData: Send, Codable {
    public var symbol: String
    public var timestamp: Date      // UTC
    public var price: Double
    public var volume: UInt64
    public var bid: Double?
    public var ask: Double?
    public var open: Double?
    public var high: Double?
    public var low: Double?

    public init(
        symbol: String,
        timestamp: Date,
        price: Double,
        volume: UInt64,
        bid: Double? = nil,
        ask: Double? = nil,
        open: Double? = nil,
        high: Double? = nil,
        low: Double? = nil
    ) {
        self.symbol = symbol
        self.timestamp = timestamp
        self.price = price
        self.volume = volume
        self.bid = bid
        self.ask = ask
        self.open = open
        self.high = high
        self.low = low
    }
}

// Analysis result from the Rust engine
public struct AnalysisResult: Send, Codable {
    public var symbol: String
    public var analyzedAt: Date
    public var indicators: [IndicatorResult]
    public var riskMetrics: RiskMetrics
    public var recommendation: String  // "Buy" | "Sell" | "Hold"
    public var confidence: Double

    public init(
        symbol: String,
        analyzedAt: Date,
        indicators: [IndicatorResult],
        riskMetrics: RiskMetrics,
        recommendation: String,
        confidence: Double
    ) {
        self.symbol = symbol
        self.analyzedAt = analyzedAt
        self.indicators = indicators
        self.riskMetrics = riskMetrics
        self.recommendation = recommendation
        self.confidence = confidence
    }
}

public struct IndicatorResult: Send, Codable {
    public var name: String
    public var timestamps: [Date]
    public var values: [Double]
    public var signals: [String]  // e.g. "Buy", "Sell", "Hold"

    public init(name: String, timestamps: [Date], values: [Double], signals: [String] = []) {
        self.name = name
        self.timestamps = timestamps
        self.values = values
        self.signals = signals
    }
}

public struct RiskMetrics: Send, Codable {
    public var volatility: Double
    public var sharpeRatio: Double?
    public var maxDrawdown: Double
    public var beta: Double?

    public init(volatility: Double, sharpeRatio: Double? = nil, maxDrawdown: Double, beta: Double? = nil) {
        self.volatility = volatility
        self.sharpeRatio = sharpeRatio
        self.maxDrawdown = maxDrawdown
        self.beta = beta
    }
}