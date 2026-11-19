// Running mean and variance of the observation, per entry, merged batch by batch (Chan et al.).
// Its mean and standard deviation are what the model file's normalisation carries.

import Foundation

public struct RunningStats: Equatable {
    public private(set) var count: Double
    public private(set) var mean: [Double]
    /// Sum of squared differences from the mean.
    private var m2: [Double]
    /// The smallest standard deviation used to normalise: an entry that barely moves in training
    /// (a flag that is always 0 in one task) is not blown up into a huge input elsewhere.
    public var stdFloor: Double

    public init(width: Int, stdFloor: Double = 0.05) {
        count = 0
        mean = [Double](repeating: 0, count: width)
        m2 = [Double](repeating: 0, count: width)
        self.stdFloor = stdFloor
    }

    /// Starts from a known mean and standard deviation, as if `count` samples had been seen.
    public init(mean: [Float], std: [Float], count: Double, stdFloor: Double = 0.05) {
        self.count = count
        self.mean = mean.map(Double.init)
        m2 = std.map { Double($0) * Double($0) * count }
        self.stdFloor = stdFloor
    }

    public var width: Int { mean.count }

    public var variance: [Double] { m2.map { count > 0 ? $0 / count : 1 } }

    /// Adds `rows` samples laid out row by row in `batch` (`rows * width` values).
    public mutating func update(_ batch: [Float], rows: Int) {
        precondition(batch.count == rows * width)
        guard rows > 0 else { return }
        let n = Double(rows)
        let total = count + n
        for j in 0..<width {
            var sum = 0.0
            for r in 0..<rows { sum += Double(batch[r * width + j]) }
            let batchMean = sum / n
            var squares = 0.0
            for r in 0..<rows {
                let d = Double(batch[r * width + j]) - batchMean
                squares += d * d
            }
            let delta = batchMean - mean[j]
            mean[j] += delta * n / total
            m2[j] += squares + delta * delta * count * n / total
        }
        count = total
    }

    /// The mean and standard deviation as written into the model file.
    public func normalization() -> (mean: [Float], std: [Float]) {
        (mean.map { Float($0) }, variance.map { Float(Swift.max($0.squareRoot(), stdFloor)) })
    }

    /// Normalises `rows` samples in place, exactly as the model file's reader does.
    public static func normalize(_ batch: inout [Float], rows: Int, mean: [Float], std: [Float]) {
        let w = mean.count
        for r in 0..<rows {
            for j in 0..<w {
                batch[r * w + j] = (batch[r * w + j] - mean[j]) / Swift.max(std[j], 1e-6)
            }
        }
    }
}
