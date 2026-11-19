// A training run: the session with the server, rollouts, updates, the log, checkpoints.

import Foundation
import MLX
import TrainerCore

/// How to reach a server: one given by address, or one started here.
struct Connection {
    var port: UInt16?
    var unixPath: String?
    var serverBinary = ServerProcess.defaultBinary()
    var threads: Int?

    /// The client, and the server process if one was started (it stops with the trainer).
    func open() throws -> (EnvClient, ServerProcess?) {
        if let unixPath { return (try EnvClient(unixPath: unixPath), nil) }
        if let port { return (try EnvClient(port: port), nil) }
        let server = try ServerProcess(binary: serverBinary, threads: threads)
        return (try EnvClient(unixPath: server.socketPath), server)
    }
}

struct TrainOptions {
    var task = "hold"
    var ppo = PPOConfig()
    var actionMode = "incremental"
    var flaps = false
    var trim = false
    var params: [String: Double] = [:]
    var initModel: String?
    var freshCritic = false
    /// With --init: keep adapting the observation normalisation.
    var updateStats = false
    var out = "runs/run"
    var checkpointEvery = 25
    var connection = Connection()
    /// The command line, written next to the run.
    var commandLine = ""
}

/// Tallies of the episodes that ended, by reason.
struct EpisodeTally {
    var count = 0
    var returnSum = 0.0
    var lengthSum = 0
    var reasons: [Int]

    init(reasons: Int) { self.reasons = [Int](repeating: 0, count: reasons) }

    mutating func add(_ e: EpisodeEnd) {
        count += 1
        returnSum += Double(e.episodeReturn)
        lengthSum += e.length
        if e.reason < reasons.count { reasons[e.reason] += 1 }
    }

    var meanReturn: Double { count > 0 ? returnSum / Double(count) : .nan }
    var meanLength: Double { count > 0 ? Double(lengthSum) / Double(count) : .nan }

    func share(_ reason: Int) -> Double { count > 0 ? Double(reasons[reason]) / Double(count) : .nan }

    func endings(_ names: [String]) -> String {
        zip(names, reasons).filter { $0.1 > 0 }.map { "\($0.0) \($0.1)" }.joined(separator: ", ")
    }
}

func train(_ o: TrainOptions) throws {
    var config = o.ppo
    var hello: [String: Any] = [
        "task": o.task, "num_envs": config.envs, "action_mode": o.actionMode, "flaps": o.flaps, "trim": o.trim,
        "params": o.params,
    ]
    var initial: ModelFile?
    if let path = o.initModel {
        let m = try ModelFile.load(path)
        config.freezeStats = !o.updateStats && m.mean != nil
        // The network and the actions are the model's.
        config.hidden = Array(m.sizes.dropFirst().dropLast())
        config.relu = m.activation == "relu"
        hello["action_mode"] = m.actionMode
        hello["flaps"] = m.actions.contains("flaps")
        hello["trim"] = m.actions.contains("trim")
        hello["decision_dt"] = m.decisionDt
        initial = m
    }
    if let threads = o.connection.threads { hello["threads"] = threads }

    let (client, server) = try o.connection.open()
    defer {
        client.close()
        server?.stop()
    }
    let d = try client.hello(hello)
    let (n, obsLen, actLen) = (d.numEnvs, d.observationLength, d.actionLength)
    print("\(d.server): \(d.task), \(n) environments on \(d.threads) threads, time limit \(d.timeLimit) s, \(obsLen) observations, \(actLen) \(d.actionMode) actions")
    if !o.params.isEmpty {
        print("set: " + o.params.sorted { $0.key < $1.key }.map { "\($0.key)=\($0.value)" }.joined(separator: " "))
    }

    let ppo = PPO(config: config, observations: obsLen, actions: actLen)
    if let m = initial, let path = o.initModel {
        guard m.observation == d.observationNames, m.actions == d.actionNames else {
            throw TrainerError("--init: \(path) was trained on another observation or action layout")
        }
        try ppo.net.loadActor(m)
        if let mean = m.mean, let std = m.std {
            // Frozen unless --update-stats: then as if it had seen a few rollouts.
            ppo.stats = RunningStats(mean: mean, std: std, count: Double(4 * n * config.rolloutSteps), stdFloor: config.stdFloor)
        }
        let critic = criticPath(path)
        if !o.freshCritic, FileManager.default.fileExists(atPath: critic) {
            try ppo.net.loadCritic(from: critic)
            print("init: \(path) and its critic")
        } else {
            print("init: \(path), new critic" + (config.criticWarmup > 0 ? ", \(config.criticWarmup) updates of critic only" : ""))
        }
        eval(ppo.net)
    }

    // The run directory: the command line, the log, the checkpoints.
    try FileManager.default.createDirectory(atPath: o.out, withIntermediateDirectories: true)
    try (o.commandLine + "\n").write(toFile: o.out + "/command.txt", atomically: true, encoding: .utf8)
    let reasonNames = Array(d.reasons.dropFirst())  // reasons[0] is "running"
    let csvPath = o.out + "/progress.csv"
    FileManager.default.createFile(atPath: csvPath, contents: nil)
    let csv = try FileHandle(forWritingTo: URL(fileURLWithPath: csvPath))
    defer { try? csv.close() }
    func writeLine(_ s: String) { csv.write(Data((s + "\n").utf8)) }
    writeLine(
        (["update", "steps", "wall_s", "episodes", "mean_return", "mean_length", "success_rate"] + reasonNames
            + ["policy_loss", "value_loss", "entropy", "approx_kl", "clip_fraction", "grad_norm", "epochs", "learning_rate",
                "action_std", "steps_per_s", "rollout_s", "env_s", "forward_s", "update_s"]).joined(separator: ","))

    let t = config.rolloutSteps
    let batchSize = t * n
    let updates = max(1, config.totalSteps / batchSize)
    let successIndex = d.reasons.firstIndex(of: "success") ?? -1
    print("\(updates) updates of \(t) steps x \(n) environments = \(batchSize) samples, \(config.epochs) epochs of \(config.minibatches) minibatches")

    var obs = try client.reset(seed: config.seed * 1_000_000, stagger: true)
    if !config.freezeStats { ppo.stats.update(obs, rows: n) }
    var bObs = [Float](repeating: 0, count: batchSize * obsLen)
    var bAct = [Float](repeating: 0, count: batchSize * actLen)
    var bLogp = [Float](repeating: 0, count: batchSize)
    var bRew = [Float](repeating: 0, count: batchSize)
    var bVal = [Float](repeating: 0, count: batchSize)
    var bFlags = [UInt8](repeating: 0, count: batchSize)
    var bFinal = [Float](repeating: 0, count: batchSize)
    var send = [Float](repeating: 0, count: n * actLen)
    let halfLog2Pi = Float(0.5 * log(2 * Double.pi))
    let started = Date()
    var steps = 0
    var best = -Double.infinity
    var recent: [Double] = []  // mean returns of the last updates, for "best"

    for u in 0..<updates {
        let rolloutStart = Date()
        var tally = EpisodeTally(reasons: d.reasons.count)
        var envTime = 0.0  // waiting for the server
        var forwardTime = 0.0  // the networks during the rollout
        let logStd = ppo.logStd
        let std = logStd.map { exp($0) }
        for step in 0..<t {
            let x = ppo.normalized(obs, rows: n)
            let base = step * n
            bObs.replaceSubrange(base * obsLen..<(base + n) * obsLen, with: x)
            let forwardStart = Date()
            let (mean, value) = ppo.forward(x, rows: n)
            forwardTime += Date().timeIntervalSince(forwardStart)
            for i in 0..<n {
                var logp: Float = 0
                for j in 0..<actLen {
                    let k = i * actLen + j
                    let e = ppo.generator.normal()
                    let a = mean[k] + std[j] * e
                    bAct[base * actLen + k] = a
                    send[k] = Swift.min(Swift.max(a, -1), 1)
                    logp += -0.5 * e * e - logStd[j] - halfLog2Pi
                }
                bLogp[base + i] = logp
                bVal[base + i] = value[i]
            }
            let envStart = Date()
            let r = try client.step(send)
            envTime += Date().timeIntervalSince(envStart)
            for i in 0..<n {
                bRew[base + i] = r.rewards[i] * config.rewardScale
                bFlags[base + i] = r.flags[i]
            }
            // Truncated episodes are bootstrapped from the value of their last observation.
            let truncated = r.ended.filter { r.flags[$0.env] == 2 }
            if !truncated.isEmpty {
                let last = ppo.normalized(truncated.flatMap(\.lastObservation), rows: truncated.count)
                for (e, v) in zip(truncated, ppo.values(last, rows: truncated.count)) { bFinal[base + e.env] = v }
            }
            for e in r.ended { tally.add(e) }
            obs = r.observations
            if !config.freezeStats { ppo.stats.update(obs, rows: n) }
        }
        let lastValues = ppo.values(ppo.normalized(obs, rows: n), rows: n)
        let (advantages, returns) = generalizedAdvantages(
            rewards: bRew, values: bVal, flags: bFlags, finalValues: bFinal, lastValues: lastValues, n: n,
            gamma: config.gamma, lambda: config.lambda)
        let rolloutTime = Date().timeIntervalSince(rolloutStart)

        let updateStart = Date()
        let progress = Float(u) / Float(updates)
        let lr = config.learningRate + (config.learningRateEnd - config.learningRate) * progress
        let s = ppo.update(
            PPO.Batch(observations: bObs, actions: bAct, logProbs: bLogp, advantages: advantages, returns: returns),
            learningRate: lr, criticOnly: u < config.criticWarmup)
        let updateTime = Date().timeIntervalSince(updateStart)
        steps += batchSize

        // The log.
        let wall = Date().timeIntervalSince(started)
        let sps = Double(batchSize) / (rolloutTime + updateTime)
        let actionStd = ppo.logStd.map { exp(Double($0)) }.reduce(0, +) / Double(actLen)
        let success = successIndex >= 0 ? tally.share(successIndex) : .nan
        func f(_ x: Double, _ digits: Int = 3) -> String { x.isNaN ? "" : String(format: "%.\(digits)f", x) }
        writeLine(
            ([String(u + 1), String(steps), f(wall, 1), String(tally.count), f(tally.meanReturn), f(tally.meanLength, 1),
                f(success)] + reasonNames.indices.map { String(tally.reasons[$0 + 1]) }
                + [f(Double(s.policyLoss), 5), f(Double(s.valueLoss), 5), f(Double(s.entropy)), f(Double(s.approxKL), 5),
                    f(Double(s.clipFraction)), f(Double(s.gradNorm)), String(s.epochsDone), String(format: "%.3g", lr),
                    f(actionStd), f(sps, 0), f(rolloutTime, 2), f(envTime, 2), f(forwardTime, 2), f(updateTime, 2)]).joined(separator: ","))
        csv.synchronizeFile()
        print(String(
            format: "%4d/%d %9d steps %6.1f min | return %7.2f length %6.1f success %3.0f%% (%d ended: %@) | pl %+.4f vl %.4f kl %.4f clip %.2f std %.3f | %.0f sps",
            u + 1, updates, steps, wall / 60, tally.meanReturn, tally.meanLength, (success.isNaN ? 0 : success) * 100,
            tally.count, tally.endings(d.reasons), s.policyLoss, s.valueLoss, s.approxKL, s.clipFraction, actionStd, sps))

        // Checkpoints: the latest, the best by recent training return, and one every so often.
        if tally.count > 0 { recent.append(tally.meanReturn) }
        if recent.count > 5 { recent.removeFirst() }
        let model = ppo.net.modelFile(task: o.task, description: d, normalization: ppo.stats.normalization())
        func save(_ name: String) throws {
            let path = o.out + "/" + name
            try model.save(to: path)
            try ppo.net.saveCritic(to: criticPath(path))
        }
        let smoothed = recent.isEmpty ? -Double.infinity : recent.reduce(0, +) / Double(recent.count)
        if recent.count >= 3 && smoothed > best && u >= config.criticWarmup {
            best = smoothed
            try save("best.fltmodel")
        }
        if (u + 1) % o.checkpointEvery == 0 || u == updates - 1 {
            try save("model.fltmodel")
            try save(String(format: "checkpoint-%09d.fltmodel", steps))
        }
    }
    let wall = Date().timeIntervalSince(started)
    print(String(format: "done: %d steps in %.1f min, %.0f steps per second; models in %@", steps, wall / 60, Double(steps) / wall, o.out))
}
