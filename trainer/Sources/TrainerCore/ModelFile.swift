// The model file of PROTOCOL.md ("Model file"): what the simulator's `ModelPilot` flies. Written
// after training, read back for evaluation and to continue training (`--init`), and run here with
// the same arithmetic as the Rust reader so that an evaluation in Swift flies the same flight.

import Foundation

public struct ModelFile: Equatable {
    public struct Layer: Equatable {
        public var inputs: Int
        public var outputs: Int
        /// `outputs * inputs`, row by row: `weights[o * inputs + i]`.
        public var weights: [Float]
        public var bias: [Float]

        public init(inputs: Int, outputs: Int, weights: [Float], bias: [Float]) {
            self.inputs = inputs
            self.outputs = outputs
            self.weights = weights
            self.bias = bias
        }
    }

    public var task: String
    public var observation: [String]
    public var actions: [String]
    public var actionMode: String
    public var decisionDt: Double
    /// `tanh` or `relu`.
    public var activation: String
    /// `tanh` or `clip`.
    public var output: String
    /// Observation normalisation, `(x - mean) / max(std, 1e-6)`, or nil.
    public var mean: [Float]?
    public var std: [Float]?
    public var layers: [Layer]

    public init(
        task: String, observation: [String], actions: [String], actionMode: String, decisionDt: Double,
        activation: String, output: String, mean: [Float]?, std: [Float]?, layers: [Layer]
    ) {
        self.task = task
        self.observation = observation
        self.actions = actions
        self.actionMode = actionMode
        self.decisionDt = decisionDt
        self.activation = activation
        self.output = output
        self.mean = mean
        self.std = std
        self.layers = layers
    }

    public var sizes: [Int] { [layers[0].inputs] + layers.map(\.outputs) }

    // MARK: Writing

    static let magic = Array("FLTMODEL".utf8)

    func header() -> String {
        func string(_ s: String) -> String {
            var out = "\""
            for c in s.unicodeScalars {
                switch c {
                case "\"": out += "\\\""
                case "\\": out += "\\\\"
                case _ where c.value < 0x20: out += String(format: "\\u%04x", c.value)
                default: out.unicodeScalars.append(c)
                }
            }
            return out + "\""
        }
        func strings(_ v: [String]) -> String { "[" + v.map(string).joined(separator: ",") + "]" }
        return "{\"format\":1,\"task\":\(string(task)),\"observation\":\(strings(observation)),"
            + "\"actions\":\(strings(actions)),\"action_mode\":\(string(actionMode)),"
            + "\"decision_dt\":\(decisionDt),\"sizes\":[\(sizes.map(String.init).joined(separator: ","))],"
            + "\"activation\":\(string(activation)),\"output\":\(string(output)),"
            + "\"normalize\":\(mean != nil)}"
    }

    public func bytes() -> [UInt8] {
        let h = Array(header().utf8)
        var out = ModelFile.magic
        let n = UInt32(h.count)
        out += [UInt8(n & 0xFF), UInt8(n >> 8 & 0xFF), UInt8(n >> 16 & 0xFF), UInt8(n >> 24)]
        out += h
        func put(_ v: [Float]) {
            for x in v {
                let b = x.bitPattern.littleEndian
                out += [UInt8(b & 0xFF), UInt8(b >> 8 & 0xFF), UInt8(b >> 16 & 0xFF), UInt8(b >> 24)]
            }
        }
        if let mean, let std {
            put(mean)
            put(std)
        }
        for l in layers {
            put(l.weights)
            put(l.bias)
        }
        return out
    }

    /// Checks the shapes and writes the file (through a temporary file, so a reader never sees
    /// half of one).
    public func save(to path: String) throws {
        try validate()
        let temporary = path + ".tmp"
        try Data(bytes()).write(to: URL(fileURLWithPath: temporary))
        if FileManager.default.fileExists(atPath: path) { try FileManager.default.removeItem(atPath: path) }
        try FileManager.default.moveItem(atPath: temporary, toPath: path)
    }

    public func validate() throws {
        guard !layers.isEmpty else { throw TrainerError("model: no layers") }
        var width = observation.count
        for (k, l) in layers.enumerated() {
            if l.inputs != width || l.weights.count != l.inputs * l.outputs || l.bias.count != l.outputs {
                throw TrainerError("model: layer \(k) has the wrong shape")
            }
            width = l.outputs
        }
        if width != actions.count { throw TrainerError("model: \(width) outputs for \(actions.count) actions") }
        if (mean?.count ?? observation.count) != observation.count || (std?.count ?? observation.count) != observation.count
            || (mean == nil) != (std == nil)
        {
            throw TrainerError("model: normalisation of the wrong length")
        }
        let all = (mean ?? []) + (std ?? []) + layers.flatMap { $0.weights + $0.bias }
        if !all.allSatisfy(\.isFinite) { throw TrainerError("model: a number is not finite") }
    }

    // MARK: Reading

    public static func load(_ path: String) throws -> ModelFile {
        guard let data = FileManager.default.contents(atPath: path) else { throw TrainerError("cannot read \(path)") }
        return try parse(Array(data))
    }

    public static func parse(_ bytes: [UInt8]) throws -> ModelFile {
        guard bytes.count >= 12, Array(bytes[0..<8]) == magic else { throw TrainerError("not a flight model file") }
        let headerLength = Int(bytes.u32(8))
        guard bytes.count >= 12 + headerLength,
            let h = try JSONSerialization.jsonObject(with: Data(bytes[12..<12 + headerLength])) as? [String: Any],
            h["format"] as? Int == 1,
            let task = h["task"] as? String, let observation = h["observation"] as? [String],
            let actions = h["actions"] as? [String], let actionMode = h["action_mode"] as? String,
            let decisionDt = h["decision_dt"] as? Double, let sizes = h["sizes"] as? [Int], sizes.count >= 2,
            let activation = h["activation"] as? String, let output = h["output"] as? String
        else { throw TrainerError("model file: bad header") }
        let normalize = h["normalize"] as? Bool ?? false
        var at = 12 + headerLength
        func take(_ n: Int) throws -> [Float] {
            guard at + 4 * n <= bytes.count else { throw TrainerError("model file: truncated") }
            defer { at += 4 * n }
            return bytes.floats(at, n)
        }
        let mean = normalize ? try take(sizes[0]) : nil
        let std = normalize ? try take(sizes[0]) : nil
        var layers: [Layer] = []
        for k in 0..<sizes.count - 1 {
            let (i, o) = (sizes[k], sizes[k + 1])
            layers.append(Layer(inputs: i, outputs: o, weights: try take(i * o), bias: try take(o)))
        }
        if at != bytes.count { throw TrainerError("model file: \(bytes.count - at) bytes after the numbers") }
        let model = ModelFile(
            task: task, observation: observation, actions: actions, actionMode: actionMode, decisionDt: decisionDt,
            activation: activation, output: output, mean: mean, std: std, layers: layers)
        try model.validate()
        return model
    }

    // MARK: Running it

    /// The action for one observation, computed in the order the Rust pilot computes it
    /// (`crates/sim/sim-env/src/model.rs`, `Model::act`), so that both give the same bits.
    public func act(_ obs: ArraySlice<Float>) -> [Float] {
        var x: [Float]
        if let mean, let std {
            x = (0..<obs.count).map { (obs[obs.startIndex + $0] - mean[$0]) / Swift.max(std[$0], 1e-6) }
        } else {
            x = Array(obs)
        }
        let relu = activation == "relu"
        for (k, l) in layers.enumerated() {
            var y = [Float](repeating: 0, count: l.outputs)
            for o in 0..<l.outputs {
                var sum = l.bias[o]
                let row = o * l.inputs
                for i in 0..<l.inputs { sum = sum + l.weights[row + i] * x[i] }
                if k == layers.count - 1 {
                    y[o] = output == "tanh" ? tanhf(sum) : Swift.min(Swift.max(sum, -1), 1)
                } else {
                    y[o] = relu ? Swift.max(sum, 0) : tanhf(sum)
                }
            }
            x = y
        }
        return x
    }
}
