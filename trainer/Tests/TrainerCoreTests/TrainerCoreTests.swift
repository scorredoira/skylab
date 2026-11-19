import Foundation
import XCTest

@testable import TrainerCore

final class AdvantageTests: XCTestCase {
    /// Two environments, three steps, worked out by hand. Environment 0 runs through and is
    /// bootstrapped from the value after the rollout; environment 1 terminates at step 0 and is
    /// truncated at step 1 (bootstrapped from its final observation's value, 4).
    func testHandComputedExample() {
        let n = 2
        // index t * n + e
        let rewards: [Float] = [1, 1, 2, 1, 3, 1]
        let values: [Float] = [0.5, 1, 1.0, 1, 1.5, 1]
        let flags: [UInt8] = [0, 1, 0, 2, 0, 0]
        let finalValues: [Float] = [0, 0, 0, 4, 0, 0]
        let (advantages, returns) = generalizedAdvantages(
            rewards: rewards, values: values, flags: flags, finalValues: finalValues, lastValues: [2, 0], n: n,
            gamma: 0.9, lambda: 0.8)
        // Environment 0: δ2 = 3 + 0.9·2 − 1.5 = 3.3, δ1 = 2 + 0.9·1.5 − 1 = 2.35, δ0 = 1 + 0.9·1 − 0.5 = 1.4;
        // A2 = 3.3, A1 = 2.35 + 0.72·3.3 = 4.726, A0 = 1.4 + 0.72·4.726 = 4.80272.
        // Environment 1: A2 = 1 + 0 − 1 = 0 (bootstrapped from 0), A1 = 1 + 0.9·4 − 1 = 3.6 (no
        // continuation past the truncation), A0 = 1 − 1 = 0 (terminated).
        let expected: [Float] = [4.80272, 0, 4.726, 3.6, 3.3, 0]
        for (a, e) in zip(advantages, expected) { XCTAssertEqual(a, e, accuracy: 1e-5) }
        for i in 0..<6 { XCTAssertEqual(returns[i], advantages[i] + values[i], accuracy: 1e-6) }
    }

    /// With λ = 1 and no endings the advantage is the discounted return minus the value.
    func testLambdaOneIsMonteCarlo() {
        let rewards: [Float] = [1, 0, 2, 5]
        let (advantages, _) = generalizedAdvantages(
            rewards: rewards, values: [0, 0, 0, 0], flags: [0, 0, 0, 0], finalValues: [0, 0, 0, 0], lastValues: [10],
            n: 1, gamma: 0.5, lambda: 1)
        XCTAssertEqual(advantages[0], 1 + 0.5 * 0 + 0.25 * 2 + 0.125 * 5 + 0.0625 * 10, accuracy: 1e-6)
        XCTAssertEqual(advantages[3], 5 + 0.5 * 10, accuracy: 1e-6)
    }
}

final class RunningStatsTests: XCTestCase {
    func testMatchesDirectComputation() {
        var g = Generator(seed: 3)
        let width = 3
        var all: [[Float]] = []
        var stats = RunningStats(width: width, stdFloor: 0)
        for rows in [5, 1, 17, 40] {
            var batch: [Float] = []
            for _ in 0..<rows {
                let row: [Float] = [g.normal() * 2 + 1, Float(g.uniform()) * 10, 7]
                all.append(row)
                batch += row
            }
            stats.update(batch, rows: rows)
        }
        XCTAssertEqual(stats.count, Double(all.count))
        for j in 0..<width {
            let column = all.map { Double($0[j]) }
            let mean = column.reduce(0, +) / Double(column.count)
            let variance = column.map { ($0 - mean) * ($0 - mean) }.reduce(0, +) / Double(column.count)
            XCTAssertEqual(stats.mean[j], mean, accuracy: 1e-9)
            XCTAssertEqual(stats.variance[j], variance, accuracy: 1e-9)
        }
    }

    func testFloorAndRestart() {
        var stats = RunningStats(width: 2, stdFloor: 0.05)
        stats.update([1, 5, 3, 5], rows: 2)
        let (mean, std) = stats.normalization()
        XCTAssertEqual(mean, [2, 5])
        XCTAssertEqual(std, [1, 0.05])  // the constant entry gets the floor
        var batch: [Float] = [3, 5]
        RunningStats.normalize(&batch, rows: 1, mean: mean, std: std)
        XCTAssertEqual(batch, [1, 0])
        // Restarting from a model file's numbers gives the same normalisation.
        let again = RunningStats(mean: mean, std: [1, 0], count: 2, stdFloor: 0.05)
        XCTAssertEqual(again.normalization().std, std)
        XCTAssertEqual(again.normalization().mean, mean)
    }
}

final class ModelFileTests: XCTestCase {
    func small() -> ModelFile {
        ModelFile(
            task: "hold", observation: ["a", "b"], actions: ["elevator"], actionMode: "incremental", decisionDt: 0.1,
            activation: "tanh", output: "tanh", mean: [1, 0], std: [2, 0],
            layers: [
                .init(inputs: 2, outputs: 2, weights: [1, 0, 0.5, -1], bias: [0, 0.25]),
                .init(inputs: 2, outputs: 1, weights: [2, 1], bias: [-0.5]),
            ])
    }

    func testRoundTripAndSize() throws {
        let m = small()
        let bytes = m.bytes()
        XCTAssertEqual(Array(bytes[0..<8]), Array("FLTMODEL".utf8))
        let headerLength = Int(bytes.u32(8))
        XCTAssertEqual(bytes.count, 12 + headerLength + 4 * (2 + 2 + 4 + 2 + 2 + 1))
        XCTAssertEqual(try ModelFile.parse(bytes), m)
    }

    func testForwardPass() {
        // x = ((3 - 1) / 2, (0.5 - 0) / max(0, 1e-6)) = (1, 500000)
        // hidden = tanh(1), tanh(0.5 - 500000 + 0.25) = (tanh 1, -1); out = tanh(2 tanh 1 - 1 - 0.5)
        let a = small().act([3, 0.5][...])
        XCTAssertEqual(a[0], tanhf(2 * tanhf(1) - 1 - 0.5), accuracy: 1e-6)
    }

    // MARK: Against the Rust reader

    /// The `fly_model` example of crates/sim/sim-env, built in release.
    func flyModel() throws -> String {
        let env = ProcessInfo.processInfo.environment
        let path = env["FLY_MODEL"]
            ?? (env["CARGO_TARGET_DIR"].map { $0 + "/release/examples/fly_model" })
            ?? {
                var url = URL(fileURLWithPath: #filePath)
                for _ in 0..<4 { url.deleteLastPathComponent() }
                return url.appendingPathComponent("target/release/examples/fly_model").path
            }()
        guard FileManager.default.isExecutableFile(atPath: path) else {
            throw XCTSkip("no fly_model at \(path): cargo build --release -p sim-env --example fly_model")
        }
        return path
    }

    func run(_ binary: String, _ arguments: [String]) throws -> String {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: binary)
        p.arguments = arguments
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = pipe
        try p.run()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        let text = String(decoding: data, as: UTF8.self)
        XCTAssertEqual(p.terminationStatus, 0, text)
        return text
    }

    /// Writes the Rust crate's hand-made wings leveller (`model::wings_leveller`) from Swift: the
    /// file must be the same bytes as the one Rust writes, read back the same, and fly the same.
    func testLevellerMatchesTheRustWriterAndReader() throws {
        let fly = try flyModel()
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("trainer-test-\(getpid())")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let rustPath = dir.appendingPathComponent("rust.fltmodel").path
        let swiftPath = dir.appendingPathComponent("swift.fltmodel").path
        _ = try run(fly, ["--write-example", rustPath])
        let rust = try ModelFile.load(rustPath)

        let names = rust.observation
        func at(_ name: String) -> Int { names.firstIndex(of: name)! }
        var w = [Float](repeating: 0, count: 4 * names.count)
        func set(_ o: Int, _ name: String, _ v: Float) { w[o * names.count + at(name)] = v }
        set(0, "altitude_error", 0.25)
        set(0, "vertical_speed", -0.9)
        set(0, "pitch_rate", -0.6)
        set(1, "heading_error", 0.5)
        set(1, "roll", -1.0)
        set(1, "roll_rate", -0.3)
        set(2, "beta", 0.3)
        set(2, "yaw_rate", -0.3)
        set(3, "speed_error", 1.5)
        set(3, "altitude_error", 0.3)
        let ours = ModelFile(
            task: "hold", observation: names, actions: ["elevator", "aileron", "rudder", "throttle"],
            actionMode: "direct", decisionDt: 0.1, activation: "tanh", output: "clip", mean: nil, std: nil,
            layers: [.init(inputs: names.count, outputs: 4, weights: w, bias: [0, 0, 0, 0.15])])
        XCTAssertEqual(ours, rust)
        try ours.save(to: swiftPath)
        let rustBytes = Array(FileManager.default.contents(atPath: rustPath)!)
        XCTAssertEqual(ours.bytes(), rustBytes, "the Swift writer and the Rust writer disagree")

        let a = try run(fly, [rustPath, "--episodes", "3"])
        let b = try run(fly, [swiftPath, "--episodes", "3"])
        XCTAssertEqual(a, b)
        XCTAssertTrue(b.contains("mean return"), b)
    }

    /// A model with normalisation and two hidden layers written in Swift loads in Rust, and Rust
    /// flies it as the Swift forward pass predicts: same file, so the same episodes.
    func testNormalizedModelLoadsInRust() throws {
        let fly = try flyModel()
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("trainer-test-n-\(getpid())")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let leveller = dir.appendingPathComponent("leveller.fltmodel").path
        _ = try run(fly, ["--write-example", leveller])
        let names = try ModelFile.load(leveller).observation
        var g = Generator(seed: 9)
        func random(_ n: Int, _ scale: Float) -> [Float] { (0..<n).map { _ in g.normal() * scale } }
        let sizes = [names.count, 16, 8, 4]
        let model = ModelFile(
            task: "hold", observation: names, actions: ["elevator", "aileron", "rudder", "throttle"],
            actionMode: "incremental", decisionDt: 0.1, activation: "tanh", output: "tanh",
            mean: random(names.count, 0.1), std: random(names.count, 1).map { abs($0) + 0.1 },
            layers: (0..<3).map {
                .init(inputs: sizes[$0], outputs: sizes[$0 + 1], weights: random(sizes[$0] * sizes[$0 + 1], 0.2),
                    bias: random(sizes[$0 + 1], 0.1))
            })
        let path = dir.appendingPathComponent("random.fltmodel").path
        try model.save(to: path)
        XCTAssertEqual(try ModelFile.load(path), model)
        let out = try run(fly, [path, "--episodes", "2"])
        XCTAssertTrue(out.contains("hold, model: 2 episodes"), out)
    }
}
