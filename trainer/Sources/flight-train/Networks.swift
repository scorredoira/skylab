// The actor and the critic in MLX, and how the actor becomes a model file and back.

import Foundation
import MLX
import MLXNN
import TrainerCore

/// A multilayer perceptron: linear layers with `tanh` or `relu` between them, none after the last.
final class MLP: Module, UnaryLayer {
    let layers: [Linear]
    let relu: Bool

    init(sizes: [Int], relu: Bool) {
        layers = zip(sizes.dropLast(), sizes.dropFirst()).map { Linear($0, $1) }
        self.relu = relu
    }

    init(layers: [Linear], relu: Bool) {
        self.layers = layers
        self.relu = relu
    }

    func callAsFunction(_ x: MLXArray) -> MLXArray {
        var x = x
        for (k, layer) in layers.enumerated() {
            x = layer(x)
            if k < layers.count - 1 { x = relu ? MLXNN.relu(x) : tanh(x) }
        }
        return x
    }
}

/// The policy and its critic, separate networks. The action distribution is a Gaussian whose
/// mean is `tanh(actor(x))` and whose log standard deviation is a parameter of its own, the same
/// for every state. Its samples are clamped to [-1, 1] by the environment; the deterministic
/// action, the one the model file gives, is the mean (file output `tanh`).
final class ActorCritic: Module {
    let actor: MLP
    let critic: MLP
    var logStd: MLXArray

    init(observations: Int, actions: Int, hidden: [Int], relu: Bool, logStd: Float) {
        actor = MLP(sizes: [observations] + hidden + [actions], relu: relu)
        critic = MLP(sizes: [observations] + hidden + [1], relu: relu)
        self.logStd = MLXArray([Float](repeating: logStd, count: actions))
        super.init()
        // A small last layer starts the policy near "do nothing" with all of its spread coming
        // from the noise, as PPO implementations usually do.
        let last = actor.layers[actor.layers.count - 1]
        last.update(parameters: ModuleParameters.unflattened([
            ("weight", last.weight * 0.01), ("bias", MLXArray.zeros([actions])),
        ]))
    }

    /// Mean action and value.
    func callAsFunction(_ x: MLXArray) -> (mean: MLXArray, value: MLXArray) {
        (tanh(actor(x)), critic(x).squeezed(axis: -1))
    }

    // MARK: Model file

    /// The actor as a model file.
    func modelFile(
        task: String, description: Description, normalization: (mean: [Float], std: [Float])
    ) -> ModelFile {
        let layers = actor.layers.map { l -> ModelFile.Layer in
            let w = l.weight  // [out, in], row by row as the file wants
            return ModelFile.Layer(
                inputs: w.dim(1), outputs: w.dim(0), weights: w.asArray(Float.self),
                bias: l.bias!.asArray(Float.self))
        }
        return ModelFile(
            task: task, observation: description.observationNames, actions: description.actionNames,
            actionMode: description.actionMode, decisionDt: description.decisionDt,
            activation: actor.relu ? "relu" : "tanh", output: "tanh",
            mean: normalization.mean, std: normalization.std, layers: layers)
    }

    /// Replaces the actor's weights with a model file's.
    func loadActor(_ model: ModelFile) throws {
        guard model.output == "tanh" else {
            throw TrainerError("--init needs a model whose output is tanh (this one is \(model.output))")
        }
        guard model.layers.count == actor.layers.count,
            zip(model.layers, actor.layers).allSatisfy({ $0.inputs == $1.weight.dim(1) && $0.outputs == $1.weight.dim(0) })
        else { throw TrainerError("--init: the model's sizes \(model.sizes) differ from the network's") }
        var parameters: [(String, MLXArray)] = []
        for (k, l) in model.layers.enumerated() {
            parameters.append(("actor.layers.\(k).weight", MLXArray(l.weights, [l.outputs, l.inputs])))
            parameters.append(("actor.layers.\(k).bias", MLXArray(l.bias)))
        }
        update(parameters: ModuleParameters.unflattened(parameters))
    }

    // MARK: The rest of a checkpoint

    /// The critic and the log standard deviation, next to a model file, so that `--init` can
    /// continue a training where it stopped.
    func saveCritic(to path: String) throws {
        var arrays: [String: MLXArray] = ["logStd": logStd]
        for (key, value) in critic.parameters().flattened(prefix: "critic") { arrays[key] = value }
        try save(arrays: arrays, url: URL(fileURLWithPath: path))
    }

    func loadCritic(from path: String) throws {
        let arrays = try loadArrays(url: URL(fileURLWithPath: path))
        guard let logStd = arrays["logStd"], logStd.shape == self.logStd.shape else {
            throw TrainerError("\(path): no log std of the right shape")
        }
        for (key, value) in critic.parameters().flattened(prefix: "critic") {
            guard let loaded = arrays[key], loaded.shape == value.shape else {
                throw TrainerError("\(path): the critic has another shape")
            }
        }
        update(parameters: ModuleParameters.unflattened(arrays.map { ($0.key, $0.value) }))
    }
}

/// Where the critic of a model file is kept.
func criticPath(_ modelPath: String) -> String { modelPath + ".critic.safetensors" }
