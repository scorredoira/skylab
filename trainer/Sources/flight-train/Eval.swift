// Deterministic evaluation of a model file through the server: one episode per environment,
// environment i with seed `seed + i`, actions computed from the file itself with the arithmetic of
// the Rust pilot (so `fly_model` with the same seeds flies the same episodes).

import Foundation
import TrainerCore

struct EvalOptions {
    var model = ""
    var task: String?
    var episodes = 200
    var seed: UInt64 = 1
    var params: [String: Double] = [:]
    var verbose = false
    var connection = Connection()
}

func evaluate(_ o: EvalOptions) throws {
    let m = try ModelFile.load(o.model)
    var hello: [String: Any] = [
        "task": o.task ?? m.task, "num_envs": o.episodes, "action_mode": m.actionMode,
        "flaps": m.actions.contains("flaps"), "trim": m.actions.contains("trim"), "decision_dt": m.decisionDt,
        "params": o.params,
    ]
    if let threads = o.connection.threads { hello["threads"] = threads }
    let (client, server) = try o.connection.open()
    defer {
        client.close()
        server?.stop()
    }
    let d = try client.hello(hello)
    guard m.observation == d.observationNames, m.actions == d.actionNames else {
        throw TrainerError("\(o.model) was trained on another observation or action layout")
    }
    let (n, obsLen, actLen) = (d.numEnvs, d.observationLength, d.actionLength)
    var obs = try client.reset(seed: o.seed)
    var first = [EpisodeEnd?](repeating: nil, count: n)
    var left = n
    var actions = [Float](repeating: 0, count: n * actLen)
    let started = Date()
    var steps = 0
    while left > 0 {
        let chunks = 8
        let current = obs
        actions.withUnsafeMutableBufferPointer { out in
            DispatchQueue.concurrentPerform(iterations: chunks) { c in
                for i in stride(from: c, to: n, by: chunks) where first[i] == nil {
                    let a = m.act(current[i * obsLen..<(i + 1) * obsLen])
                    for j in 0..<actLen { out[i * actLen + j] = a[j] }
                }
            }
        }
        let r = try client.step(actions)
        steps += n
        for e in r.ended where first[e.env] == nil {
            first[e.env] = e
            left -= 1
        }
        obs = r.observations
    }
    let elapsed = Date().timeIntervalSince(started)
    var tally = EpisodeTally(reasons: d.reasons.count)
    // How far from its targets each episode ended, in the units of the field descriptions.
    func raw(_ e: EpisodeEnd, _ name: String) -> Double {
        guard let k = d.observationNames.firstIndex(of: name) else { return .nan }
        return Double(e.lastObservation[k]) * d.observationScales[k]
    }
    var finalErrors: [[Double]] = []
    for (i, e) in first.enumerated() {
        guard let e else { continue }
        tally.add(e)
        let errors = [abs(raw(e, "heading_error")) * 180 / .pi, abs(raw(e, "altitude_error")), abs(raw(e, "speed_error"))]
        finalErrors.append(errors)
        if o.verbose {
            print(String(format: "episode %d: seed %llu, return %8.2f, %6.1f s, %@; at the end off by %.1f°, %.1f m, %.1f m/s",
                i, o.seed + UInt64(i), e.episodeReturn, Double(e.length) * d.decisionDt, d.reasons[e.reason],
                errors[0], errors[1], errors[2]))
        }
    }
    if !finalErrors.isEmpty {
        func median(_ k: Int) -> Double { finalErrors.map { $0[k] }.sorted()[finalErrors.count / 2] }
        print(String(format: "median error at the end: heading %.1f°, altitude %.1f m, airspeed %.1f m/s", median(0), median(1), median(2)))
    }
    let success = d.reasons.firstIndex(of: "success").map { tally.share($0) * 100 } ?? 0
    let set = o.params.isEmpty ? "" : " [" + o.params.sorted { $0.key < $1.key }.map { "\($0.key)=\($0.value)" }.joined(separator: " ") + "]"
    print(String(format: "%@ on %@%@: %d episodes, mean return %.2f, mean length %.1f s, success %.1f%% [%@] (%.1f s)",
        (o.model as NSString).lastPathComponent, d.task, set, tally.count, tally.meanReturn, tally.meanLength * d.decisionDt,
        success, tally.endings(d.reasons), elapsed))
}
