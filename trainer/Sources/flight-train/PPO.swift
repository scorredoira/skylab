// Proximal policy optimisation: rollouts from the vectorised environment, GAE, and minibatch
// epochs of the clipped surrogate with a value loss and an entropy bonus.

import Foundation
import MLX
import MLXNN
import MLXOptimizers
import TrainerCore

struct PPOConfig {
    var envs = 256
    var totalSteps = 5_000_000
    /// Decisions per environment in one rollout.
    var rolloutSteps = 64
    var epochs = 5
    var minibatches = 4
    var learningRate: Float = 3e-4
    /// The learning rate falls linearly to this at the end of the run.
    var learningRateEnd: Float = 0
    var gamma: Float = 0.99
    var lambda: Float = 0.95
    var clip: Float = 0.2
    var valueCoef: Float = 0.5
    var entropyCoef: Float = 0
    var maxGradNorm: Float = 0.5
    /// Stop an update's epochs early once the KL to the rollout's policy passes this (0: never).
    var targetKL: Float = 0
    /// Rewards are multiplied by this for learning (the reported returns are not).
    var rewardScale: Float = 1
    var hidden = [128, 128]
    var relu = false
    var logStdInit: Float = -0.5
    var seed: UInt64 = 1
    /// Updates at the start in which only the critic learns (for `--init` without its critic).
    var criticWarmup = 0
    /// Keep the observation normalisation as it is (the default with `--init`: the model was
    /// trained on those numbers, and statistics of the first rollouts of a new run would shift them).
    var freezeStats = false
    var stdFloor = 0.05
}

/// What one update did, for the log and `progress.csv`.
struct UpdateStats {
    var policyLoss: Float = 0
    var valueLoss: Float = 0
    var entropy: Float = 0
    var approxKL: Float = 0
    var clipFraction: Float = 0
    var gradNorm: Float = 0
    var epochsDone = 0
}

final class PPO {
    let config: PPOConfig
    let net: ActorCritic
    let optimizer: Adam
    var stats: RunningStats
    var generator: Generator
    let observations: Int
    let actions: Int

    init(config: PPOConfig, observations: Int, actions: Int) {
        self.config = config
        self.observations = observations
        self.actions = actions
        MLXRandom.seed(config.seed)
        net = ActorCritic(
            observations: observations, actions: actions, hidden: config.hidden, relu: config.relu,
            logStd: config.logStdInit)
        eval(net)
        optimizer = Adam(learningRate: config.learningRate, eps: 1e-5, biasCorrection: true)
        stats = RunningStats(width: observations, stdFloor: config.stdFloor)
        generator = Generator(seed: config.seed &* 0x9E37_79B9 &+ 17)
    }

    // MARK: Acting

    /// Mean action and value for `rows` normalised observations.
    func forward(_ x: [Float], rows: Int) -> (mean: [Float], value: [Float]) {
        let (mean, value) = net(MLXArray(x, [rows, observations]))
        eval(mean, value)
        return (mean.asArray(Float.self), value.asArray(Float.self))
    }

    func values(_ x: [Float], rows: Int) -> [Float] {
        let v = net.critic(MLXArray(x, [rows, observations])).squeezed(axis: -1)
        eval(v)
        return v.asArray(Float.self)
    }

    func normalized(_ raw: [Float], rows: Int) -> [Float] {
        var x = raw
        let (mean, std) = stats.normalization()
        RunningStats.normalize(&x, rows: rows, mean: mean, std: std)
        return x
    }

    // MARK: Learning

    /// One rollout's worth of samples, `B = T * N` of each.
    struct Batch {
        var observations: [Float]  // B * O, normalised
        var actions: [Float]  // B * A, as sampled (before clamping)
        var logProbs: [Float]  // B
        var advantages: [Float]  // B
        var returns: [Float]  // B
    }

    /// The PPO epochs over one batch.
    func update(_ batch: Batch, learningRate: Float, criticOnly: Bool) -> UpdateStats {
        optimizer.learningRate = learningRate
        let size = batch.logProbs.count
        let obs = MLXArray(batch.observations, [size, observations])
        let act = MLXArray(batch.actions, [size, actions])
        let oldLogp = MLXArray(batch.logProbs)
        let adv = MLXArray(batch.advantages)
        let ret = MLXArray(batch.returns)
        let clipRange = config.clip
        let valueCoef = config.valueCoef
        let entropyCoef = config.entropyCoef
        let policyWeight: Float = criticOnly ? 0 : 1
        let halfLog2Pi = Float(0.5 * log(2 * Double.pi))

        let lossAndGrad = valueAndGrad(model: net) { (net: ActorCritic, a: [MLXArray]) -> [MLXArray] in
            let (x, action, oldLogp, adv, ret) = (a[0], a[1], a[2], a[3], a[4])
            let (mean, value) = net(x)
            let logStd = net.logStd
            let z = (action - mean) / exp(logStd)
            let logp = (-0.5 * z * z - logStd - halfLog2Pi).sum(axis: -1)
            let logRatio = logp - oldLogp
            let ratio = exp(logRatio)
            let normAdv = (adv - adv.mean()) / (sqrt(adv.variance()) + 1e-8)
            let surrogate = minimum(ratio * normAdv, clip(ratio, min: 1 - clipRange, max: 1 + clipRange) * normAdv)
            let policyLoss = -surrogate.mean()
            let valueLoss = 0.5 * ((value - ret) * (value - ret)).mean()
            let entropy = (logStd + halfLog2Pi + 0.5).sum()
            let loss = policyWeight * (policyLoss - entropyCoef * entropy) + valueCoef * valueLoss
            let approxKL = ((ratio - 1) - logRatio).mean()
            let clipFraction = (abs(ratio - 1) .> clipRange).asType(.float32).mean()
            return [loss, policyLoss, valueLoss, entropy, approxKL, clipFraction]
        }

        var out = UpdateStats()
        var count: Float = 0
        let minibatch = size / config.minibatches
        var order = (0..<Int32(size)).map { $0 }
        for epoch in 0..<config.epochs {
            generator.shuffle(&order)
            var epochKL: Float = 0
            for m in 0..<config.minibatches {
                let idx = MLXArray(Array(order[m * minibatch..<(m + 1) * minibatch]))
                let args = [obs.take(idx, axis: 0), act.take(idx, axis: 0), oldLogp.take(idx), adv.take(idx), ret.take(idx)]
                let (values, grads) = lossAndGrad(net, args)
                let (clippedGrads, norm) = clipGradNorm(gradients: grads, maxNorm: config.maxGradNorm)
                optimizer.update(model: net, gradients: clippedGrads)
                eval(net, optimizer, values[1], values[2], values[3], values[4], values[5], norm)
                out.policyLoss += values[1].item(Float.self)
                out.valueLoss += values[2].item(Float.self)
                out.entropy += values[3].item(Float.self)
                let kl = values[4].item(Float.self)
                out.approxKL += kl
                epochKL += kl
                out.clipFraction += values[5].item(Float.self)
                out.gradNorm += norm.item(Float.self)
                count += 1
            }
            out.epochsDone = epoch + 1
            if config.targetKL > 0 && !criticOnly && epochKL / Float(config.minibatches) > config.targetKL { break }
        }
        out.policyLoss /= count
        out.valueLoss /= count
        out.entropy /= count
        out.approxKL /= count
        out.clipFraction /= count
        out.gradNorm /= count
        return out
    }

    var logStd: [Float] { net.logStd.asArray(Float.self) }
}
