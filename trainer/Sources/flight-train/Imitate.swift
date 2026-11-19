// Behaviour cloning: the actor learns to give the actions of an expert (the autopilot), from the
// observation and action records that sim-env's `race data` example writes.

import Foundation
import MLX
import MLXNN
import MLXOptimizers
import TrainerCore

struct ImitateOptions {
    var data: [String] = []
    var task = "race"
    var out = "runs/imitate"
    var initModel: String?
    var hidden = [256, 256]
    var relu = false
    var epochs = 30
    var batch = 4096
    var learningRate: Float = 1e-3
    var learningRateEnd: Float = 1e-5
    var decisionDt = 0.1
    var seed: UInt64 = 1
    var stdFloor = 0.05
    /// Expert actions are pulled in to this, so that `tanh` can reach them.
    var labelLimit: Float = 0.97
    /// Observation entries the network is not shown (their normalisation makes them 0): by default
    /// where the yoke, pedals and throttle are, which in the expert's records nearly equal the
    /// action and would teach the network to copy them instead of flying.
    /// A multirotor's task (`hover`, `gates`): its sticks instead (`multirotorMask`).
    var mask = ["elevator", "aileron", "rudder", "throttle"]
    var maskGiven = false
    var connection = Connection()
}

/// What is hidden by default from a network that flies a multirotor: where its sticks are.
let multirotorMask = ["throttle", "roll_stick", "pitch_stick", "yaw_stick"]

/// The tasks flown in a multirotor: the sticks are their actions, with no flaps or trim.
let multirotorTasks: Set<String> = ["hover", "gates"]

/// What is hidden by default from a network that drives a car (`drive`): where its steering and
/// pedals are, which with direct actions are its last action.
let carMask = ["steer", "throttle", "brake"]

/// Records of one data file: `"FLTDATA1"`, u32 observation length, u32 action length, then f32.
func readImitationData(_ path: String) throws -> (obs: Int, act: Int, values: [Float]) {
    let data = try Data(contentsOf: URL(fileURLWithPath: path))
    guard data.count >= 16, String(decoding: data.prefix(8), as: UTF8.self) == "FLTDATA1" else {
        throw TrainerError("\(path) is not a FLTDATA1 file")
    }
    func u32(_ at: Int) -> Int {
        Int(data[at]) | Int(data[at + 1]) << 8 | Int(data[at + 2]) << 16 | Int(data[at + 3]) << 24
    }
    let (o, a) = (u32(8), u32(12))
    let count = (data.count - 16) / 4
    guard count % (o + a) == 0 else { throw TrainerError("\(path): truncated") }
    var values = [Float](repeating: 0, count: count)
    _ = values.withUnsafeMutableBytes { data.copyBytes(to: $0, from: 16..<data.count) }
    return (o, a, values)
}

func imitate(_ o: ImitateOptions) throws {
    // The layout and its names come from the server, as a training's do.
    let multirotor = multirotorTasks.contains(o.task)
    let car = o.task == "drive"
    let hello: [String: Any] = [
        "task": o.task, "num_envs": 1, "action_mode": "direct", "flaps": !multirotor && !car, "trim": !multirotor && !car,
        "decision_dt": o.decisionDt,
    ]
    let mask = o.maskGiven ? o.mask : multirotor ? multirotorMask : car ? carMask : o.mask
    let (client, server) = try o.connection.open()
    let d = try client.hello(hello)
    client.close()
    server?.stop()

    var obs: [Float] = []
    var act: [Float] = []
    for path in o.data {
        let (ol, al, values) = try readImitationData(path)
        guard ol == d.observationLength, al == d.actionLength else {
            throw TrainerError("\(path) has \(ol) observations and \(al) actions, the server \(d.observationLength) and \(d.actionLength)")
        }
        let rows = values.count / (ol + al)
        obs.reserveCapacity(obs.count + rows * ol)
        act.reserveCapacity(act.count + rows * al)
        for r in 0..<rows {
            let base = r * (ol + al)
            obs.append(contentsOf: values[base..<base + ol])
            act.append(contentsOf: values[base + ol..<base + ol + al].map { Swift.min(Swift.max($0, -o.labelLimit), o.labelLimit) })
        }
        print("\(path): \(rows) decisions")
    }
    let (ol, al) = (d.observationLength, d.actionLength)
    let rows = obs.count / ol
    guard rows > o.batch else { throw TrainerError("only \(rows) decisions") }

    MLXRandom.seed(o.seed)
    let net = ActorCritic(observations: ol, actions: al, hidden: o.hidden, relu: o.relu, logStd: -1)
    var stats = RunningStats(width: ol, stdFloor: o.stdFloor)
    if let path = o.initModel {
        let m = try ModelFile.load(path)
        guard m.observation == d.observationNames, m.actions == d.actionNames else {
            throw TrainerError("--init: \(path) was trained on another observation or action layout")
        }
        try net.loadActor(m)
        if let mean = m.mean, let std = m.std {
            stats = RunningStats(mean: mean, std: std, count: 1e9, stdFloor: o.stdFloor)
        }
        print("init: \(path)")
    } else {
        stats.update(obs, rows: rows)
    }
    eval(net)
    var (mean, std) = stats.normalization()
    for name in mask {
        guard let i = d.observationNames.firstIndex(of: name) else { throw TrainerError("--mask: no observation \(name)") }
        (mean[i], std[i]) = (0, 1e6)
    }
    if !mask.isEmpty { print("not shown to the network: " + mask.joined(separator: ", ")) }
    RunningStats.normalize(&obs, rows: rows, mean: mean, std: std)

    let x = MLXArray(obs, [rows, ol])
    let y = MLXArray(act, [rows, al])
    let optimizer = Adam(learningRate: o.learningRate)
    let lossAndGrad = valueAndGrad(model: net) { (net: ActorCritic, a: [MLXArray]) -> [MLXArray] in
        let diff = tanh(net.actor(a[0])) - a[1]
        return [(diff * diff).mean()]
    }
    var generator = Generator(seed: o.seed)
    var order = (0..<Int32(rows)).map { $0 }
    let batches = rows / o.batch
    try FileManager.default.createDirectory(atPath: o.out, withIntermediateDirectories: true)
    let started = Date()
    for epoch in 0..<o.epochs {
        let progress = Float(epoch) / Float(Swift.max(1, o.epochs - 1))
        optimizer.learningRate = o.learningRate * pow(o.learningRateEnd / o.learningRate, progress)
        generator.shuffle(&order)
        var total: Float = 0
        for b in 0..<batches {
            let idx = MLXArray(Array(order[b * o.batch..<(b + 1) * o.batch]))
            let (values, grads) = lossAndGrad(net, [x.take(idx, axis: 0), y.take(idx, axis: 0)])
            optimizer.update(model: net, gradients: grads)
            eval(net, optimizer, values[0])
            total += values[0].item(Float.self)
        }
        print(String(format: "epoch %3d/%d  loss %.5f  lr %.2e  %.1f min", epoch + 1, o.epochs, total / Float(batches),
            optimizer.learningRate, Date().timeIntervalSince(started) / 60))
    }
    let model = net.modelFile(task: o.task, description: d, normalization: (mean, std))
    try model.save(to: o.out + "/model.fltmodel")
    print("model in \(o.out)/model.fltmodel")
}
