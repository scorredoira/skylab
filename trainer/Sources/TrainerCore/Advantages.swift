// Generalised advantage estimation over a rollout of T steps of N environments, and the small
// random number generator the trainer samples its actions with.

import Foundation

/// Advantages and value targets (returns) for a rollout laid out step by step: entry `t * n + e`
/// is step `t` of environment `e`.
///
/// - `values[t*n+e]`: the critic's value of the observation the action was taken in.
/// - `flags`: 0 running, 1 terminated (nothing follows), 2 truncated by the time limit.
/// - `finalValues[t*n+e]`: where truncated, the value of the episode's last observation, which
///   the step's reward is bootstrapped from; ignored elsewhere.
/// - `lastValues[e]`: the value of the observation after the last step, to bootstrap the steps
///   still running at the end of the rollout.
///
/// δ_t = r_t + γ·V(next) − V(s_t), where V(next) is 0 if terminated, the final observation's value
/// if truncated and the next step's value otherwise; A_t = δ_t + γλ·A_{t+1} within an episode.
public func generalizedAdvantages(
    rewards: [Float], values: [Float], flags: [UInt8], finalValues: [Float], lastValues: [Float],
    n: Int, gamma: Float, lambda: Float
) -> (advantages: [Float], returns: [Float]) {
    let total = rewards.count
    precondition(values.count == total && flags.count == total && finalValues.count == total && lastValues.count == n)
    precondition(total % n == 0)
    let steps = total / n
    var advantages = [Float](repeating: 0, count: total)
    for e in 0..<n {
        var next: Float = 0  // A_{t+1}
        for t in stride(from: steps - 1, through: 0, by: -1) {
            let i = t * n + e
            let nextValue: Float
            let continues: Float
            switch flags[i] {
            case 1:
                nextValue = 0
                continues = 0
            case 2:
                nextValue = finalValues[i]
                continues = 0
            default:
                nextValue = t == steps - 1 ? lastValues[e] : values[i + n]
                continues = 1
            }
            let delta = rewards[i] + gamma * nextValue - values[i]
            next = delta + gamma * lambda * continues * next
            advantages[i] = next
        }
    }
    let returns = zip(advantages, values).map { $0 + $1 }
    return (advantages, returns)
}

/// splitmix64 for seeding, xoshiro256** for the stream; normal numbers by Box–Muller.
public struct Generator {
    private var s: (UInt64, UInt64, UInt64, UInt64)
    private var spare: Float?

    public init(seed: UInt64) {
        var x = seed
        func splitmix() -> UInt64 {
            x &+= 0x9E37_79B9_7F4A_7C15
            var z = x
            z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
            z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
            return z ^ (z >> 31)
        }
        s = (splitmix(), splitmix(), splitmix(), splitmix())
    }

    public mutating func next() -> UInt64 {
        let five = s.1 &* 5
        let result = ((five << 7) | (five >> 57)) &* 9
        let t = s.1 << 17
        s.2 ^= s.0
        s.3 ^= s.1
        s.1 ^= s.2
        s.0 ^= s.3
        s.2 ^= t
        s.3 = s.3 << 45 | s.3 >> 19
        return result
    }

    /// Uniform in [0, 1).
    public mutating func uniform() -> Double { Double(next() >> 11) * 0x1.0p-53 }

    /// Standard normal.
    public mutating func normal() -> Float {
        if let z = spare {
            spare = nil
            return z
        }
        let u = 1 - uniform()  // (0, 1]
        let v = uniform()
        let r = (-2 * log(u)).squareRoot()
        spare = Float(r * sin(2 * .pi * v))
        return Float(r * cos(2 * .pi * v))
    }

    /// A random index below `n`.
    public mutating func below(_ n: Int) -> Int { Int(next() % UInt64(n)) }

    public mutating func shuffle(_ a: inout [Int32]) {
        guard a.count > 1 else { return }
        for i in stride(from: a.count - 1, to: 0, by: -1) { a.swapAt(i, below(i + 1)) }
    }
}
