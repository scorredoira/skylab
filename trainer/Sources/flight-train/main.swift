// flight-train: trains AI pilots for the flight simulator with PPO, and evaluates them.

import Foundation
import MLX
import TrainerCore

let usage = """
    flight-train: trains a pilot with PPO against the flight-env server and saves it as a model file

      flight-train [train] --task NAME [options]
      flight-train eval MODEL [--task NAME] [--episodes N] [--seed N] [--set K=V]... [--verbose]
      flight-train imitate --data FILE[,FILE...] [--out DIR] [--init MODEL] [--hidden A,B] [--epochs N]
                           [--batch N] [--lr X] [--lr-end X] [--dt S] [--mask A,B|none]

    Imitation: behaviour cloning of the records that sim-env's `race data` example writes
    (direct actions with flaps and trim; task race unless --task), or of sim-env's `gates data`
    (--task gates: the quad's four direct sticks). Writes DIR/model.fltmodel.
    Defaults: --epochs 30, --batch 4096, --lr 1e-3 falling to --lr-end 1e-5, --hidden 256,256,
    --dt 0.1 (the decision interval the records were taken at). --mask: observations hidden from the
    network (default elevator,aileron,rudder,throttle: in the records they nearly equal the action;
    for hover and gates throttle,roll_stick,pitch_stick,yaw_stick; for drive steer,throttle,brake).

    Training:
      --task NAME           hold, takeoff, landing, circuit, race, hover, gates or drive (default hold)
      --envs N              environments stepped together (default 256)
      --steps N             environment steps in all (default 5e6)
      --rollout N           steps per environment in one rollout (default 64)
      --epochs N            passes over a rollout (default 5)
      --minibatches N       per epoch (default 4)
      --lr X                learning rate at the start (default 3e-4)
      --lr-end X            ... at the end, linearly (default 0)
      --gamma X             discount per decision (default 0.99)
      --lambda X            GAE lambda (default 0.95)
      --clip X              PPO ratio clip (default 0.2)
      --vf X                value loss weight (default 0.5)
      --ent X               entropy bonus weight (default 0)
      --max-grad-norm X     gradient clipping (default 0.5)
      --target-kl X         stop an update's epochs past this KL (default 0: never)
      --reward-scale X      rewards are multiplied by this for learning (default 1)
      --hidden A,B          hidden layer widths (default 128,128)
      --activation F        tanh or relu (default tanh)
      --log-std X           initial log standard deviation of the actions (default -0.5)
      --std-floor X         smallest observation std used to normalise (default 0.05)
      --seed N              (default 1)
      --action-mode M       incremental or direct (default incremental)
      --flaps, --trim       add a flap or a trim action
      --set NAME=VALUE      a task number of the environment, e.g. config.distance_max=2000 (repeatable)
      --init MODEL          start from a model file (and its critic, if saved next to it)
      --fresh-critic        with --init: do not load the critic
      --update-stats        with --init: keep adapting the observation normalisation (default: frozen)
      --critic-warmup N     first N updates train only the critic (default 0)
      --out DIR             run directory: progress.csv, model.fltmodel, best.fltmodel, checkpoints (default runs/<task>)
      --checkpoint-every N  updates between checkpoints (default 25)

    Server (default: start one with the flight-env binary):
      --server-bin PATH     the flight-env binary (default $CARGO_TARGET_DIR/release/flight-env)
      --threads N           its worker threads (default: one per performance core)
      --port N | --unix PATH  use a server that is already running
      --device gpu|cpu      where MLX computes (default gpu)
    """

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("flight-train: \(message)\n".utf8))
    exit(2)
}

var arguments = Array(CommandLine.arguments.dropFirst())
var command = "train"
if let first = arguments.first, first == "train" || first == "eval" || first == "imitate" {
    command = first
    arguments.removeFirst()
}
if arguments.contains("--help") || arguments.contains("-h") {
    print(usage)
    exit(0)
}

var trainOptions = TrainOptions()
var evalOptions = EvalOptions()
var imitateOptions = ImitateOptions()
var connection = Connection()
var device = "gpu"
var outGiven = false
var params: [String: Double] = [:]
var task: String?

var iterator = arguments.makeIterator()
func value(_ flag: String) -> String {
    guard let v = iterator.next() else { fail("\(flag) needs a value") }
    return v
}
func int(_ flag: String) -> Int {
    guard let x = Double(value(flag)), x >= 0, x < 1e15, x == x.rounded() else { fail("bad value for \(flag)") }
    return Int(x)
}
func real(_ flag: String) -> Float {
    guard let x = Float(value(flag)), x.isFinite else { fail("bad value for \(flag)") }
    return x
}

while let flag = iterator.next() {
    switch flag {
    case "--task": task = value(flag)
    case "--set":
        let kv = value(flag).split(separator: "=", maxSplits: 1).map(String.init)
        guard kv.count == 2, let x = Double(kv[1]) else { fail("--set wants NAME=VALUE") }
        params[kv[0]] = x
    case "--seed":
        trainOptions.ppo.seed = UInt64(int(flag))
        evalOptions.seed = trainOptions.ppo.seed
    case "--server-bin": connection.serverBinary = value(flag)
    case "--threads": connection.threads = int(flag)
    case "--port": connection.port = UInt16(clamping: int(flag))
    case "--unix": connection.unixPath = value(flag)
    case "--device": device = value(flag)
    // Evaluation
    case "--episodes": evalOptions.episodes = int(flag)
    case "--verbose": evalOptions.verbose = true
    // Training
    case "--envs": trainOptions.ppo.envs = int(flag)
    case "--steps": trainOptions.ppo.totalSteps = int(flag)
    case "--rollout": trainOptions.ppo.rolloutSteps = int(flag)
    case "--epochs":
        trainOptions.ppo.epochs = int(flag)
        imitateOptions.epochs = trainOptions.ppo.epochs
    case "--data": imitateOptions.data = value(flag).split(separator: ",").map(String.init)
    case "--batch": imitateOptions.batch = max(1, int(flag))
    case "--mask":
        imitateOptions.mask = value(flag).split(separator: ",").map(String.init).filter { $0 != "none" }
        imitateOptions.maskGiven = true
    case "--dt": imitateOptions.decisionDt = Double(real(flag))
    case "--minibatches": trainOptions.ppo.minibatches = int(flag)
    case "--lr":
        trainOptions.ppo.learningRate = real(flag)
        imitateOptions.learningRate = trainOptions.ppo.learningRate
    case "--lr-end":
        trainOptions.ppo.learningRateEnd = real(flag)
        imitateOptions.learningRateEnd = trainOptions.ppo.learningRateEnd
    case "--gamma": trainOptions.ppo.gamma = real(flag)
    case "--lambda": trainOptions.ppo.lambda = real(flag)
    case "--clip": trainOptions.ppo.clip = real(flag)
    case "--vf": trainOptions.ppo.valueCoef = real(flag)
    case "--ent": trainOptions.ppo.entropyCoef = real(flag)
    case "--max-grad-norm": trainOptions.ppo.maxGradNorm = real(flag)
    case "--target-kl": trainOptions.ppo.targetKL = real(flag)
    case "--reward-scale": trainOptions.ppo.rewardScale = real(flag)
    case "--hidden":
        let sizes = value(flag).split(separator: ",").compactMap { Int($0) }
        guard !sizes.isEmpty, sizes.allSatisfy({ $0 > 0 }) else { fail("--hidden wants widths like 128,128") }
        trainOptions.ppo.hidden = sizes
        imitateOptions.hidden = sizes
    case "--activation":
        let a = value(flag)
        guard a == "tanh" || a == "relu" else { fail("--activation is tanh or relu") }
        trainOptions.ppo.relu = a == "relu"
    case "--log-std": trainOptions.ppo.logStdInit = real(flag)
    case "--std-floor": trainOptions.ppo.stdFloor = Double(real(flag))
    case "--action-mode": trainOptions.actionMode = value(flag)
    case "--flaps": trainOptions.flaps = true
    case "--trim": trainOptions.trim = true
    case "--init":
        trainOptions.initModel = value(flag)
        imitateOptions.initModel = trainOptions.initModel
    case "--fresh-critic": trainOptions.freshCritic = true
    case "--update-stats": trainOptions.updateStats = true
    case "--critic-warmup": trainOptions.ppo.criticWarmup = int(flag)
    case "--out":
        trainOptions.out = value(flag)
        outGiven = true
    case "--checkpoint-every": trainOptions.checkpointEvery = max(1, int(flag))
    default:
        if command == "eval" && !flag.hasPrefix("--") && evalOptions.model.isEmpty {
            evalOptions.model = flag
        } else {
            fail("unknown option \(flag) (see --help)")
        }
    }
}

let mlxDevice: Device
switch device {
case "gpu": mlxDevice = .gpu
case "cpu": mlxDevice = .cpu
default: fail("--device is gpu or cpu")
}

do {
    try Device.withDefaultDevice(mlxDevice) {
        if command == "imitate" {
            guard !imitateOptions.data.isEmpty else { fail("imitate needs --data") }
            if let task { imitateOptions.task = task }
            if outGiven { imitateOptions.out = trainOptions.out }
            imitateOptions.seed = trainOptions.ppo.seed
            imitateOptions.connection = connection
            try imitate(imitateOptions)
        } else if command == "eval" {
            guard !evalOptions.model.isEmpty else { fail("eval needs a model file") }
            evalOptions.task = task
            evalOptions.params = params
            evalOptions.connection = connection
            try evaluate(evalOptions)
        } else {
            trainOptions.task = task ?? "hold"
            trainOptions.params = params
            trainOptions.connection = connection
            if !outGiven { trainOptions.out = "runs/" + trainOptions.task }
            let ppo = trainOptions.ppo
            guard ppo.envs > 0, ppo.rolloutSteps > 0, ppo.epochs > 0, ppo.minibatches > 0,
                (ppo.envs * ppo.rolloutSteps) % ppo.minibatches == 0
            else { fail("envs x rollout must be a multiple of minibatches") }
            trainOptions.commandLine = CommandLine.arguments.joined(separator: " ")
            try train(trainOptions)
        }
    }
} catch {
    fail("\(error)")
}
