// The client of the flight-env server (crates/sim/sim-env/PROTOCOL.md, version 1): frames over a
// stream socket, the session messages, and starting the server as a child process.

import Foundation

public struct TrainerError: Error, CustomStringConvertible {
    public let description: String
    public init(_ description: String) { self.description = description }
}

enum MessageType: UInt8 {
    case hello = 1, describe, reset, observations, step, stepResult, close, bye, error
}

/// What `describe` said about the session.
public struct Description {
    public let numEnvs: Int
    public let threads: Int
    public let observationLength: Int
    public let observationNames: [String]
    /// Entry i of the observation is the raw value divided by this.
    public let observationScales: [Double]
    public let actionLength: Int
    public let actionNames: [String]
    public let actionMode: String
    public let decisionDt: Double
    public let task: String
    public let timeLimit: Double
    public let maxSteps: Int
    public let reasons: [String]
    /// Every task parameter by name, with the value of this session.
    public let params: [(name: String, value: Double)]
    public let server: String

    init(json: [String: Any]) throws {
        guard let observation = json["observation"] as? [String: Any],
            let action = json["action"] as? [String: Any],
            let task = json["task"] as? [String: Any],
            let reasons = json["reasons"] as? [String],
            let numEnvs = json["num_envs"] as? Int,
            let observationLength = observation["len"] as? Int,
            let actionLength = action["len"] as? Int,
            let decisionDt = json["decision_dt"] as? Double
        else { throw TrainerError("cannot read the description") }
        self.numEnvs = numEnvs
        self.threads = json["threads"] as? Int ?? 0
        self.observationLength = observationLength
        self.observationNames = (observation["fields"] as? [[String: Any]] ?? []).compactMap { $0["name"] as? String }
        self.observationScales = (observation["fields"] as? [[String: Any]] ?? []).map { $0["scale"] as? Double ?? 1 }
        self.actionLength = actionLength
        self.actionNames = (action["fields"] as? [[String: Any]] ?? []).compactMap { $0["name"] as? String }
        self.actionMode = action["mode"] as? String ?? "incremental"
        self.decisionDt = decisionDt
        self.task = task["name"] as? String ?? "?"
        self.timeLimit = task["time_limit"] as? Double ?? 0
        self.maxSteps = task["max_steps"] as? Int ?? 0
        self.reasons = reasons
        self.params = (task["params"] as? [[String: Any]] ?? []).compactMap {
            guard let name = $0["name"] as? String, let value = $0["value"] as? Double else { return nil }
            return (name, value)
        }
        self.server = json["server"] as? String ?? "?"
        if observationNames.count != observationLength || actionNames.count != actionLength {
            throw TrainerError("the description's field lists do not match its lengths")
        }
    }
}

/// An episode that ended in a step.
public struct EpisodeEnd {
    public let env: Int
    public let reason: Int
    public let episodeReturn: Float
    public let length: Int
    /// The last observation of the ended episode (to bootstrap a truncated one).
    public let lastObservation: [Float]
}

/// The answer to `step`.
public struct StepResult {
    /// `N * O`: for an environment whose episode ended, the first of its next episode.
    public var observations: [Float]
    public var rewards: [Float]
    /// 0 running, 1 terminated, 2 truncated.
    public var flags: [UInt8]
    public var ended: [EpisodeEnd]
}

/// One session with the server.
public final class EnvClient {
    private let fd: Int32
    public private(set) var description: Description?

    /// Connects to `127.0.0.1:port`.
    public init(port: UInt16) throws {
        fd = socket(AF_INET, SOCK_STREAM, 0)
        var address = sockaddr_in()
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = port.bigEndian
        address.sin_addr.s_addr = inet_addr("127.0.0.1")
        let result = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        if result != 0 {
            Foundation.close(fd)
            throw TrainerError("cannot connect to 127.0.0.1:\(port): \(String(cString: strerror(errno)))")
        }
        var one: Int32 = 1
        setsockopt(fd, Int32(IPPROTO_TCP), TCP_NODELAY, &one, socklen_t(MemoryLayout<Int32>.size))
    }

    /// Connects to a Unix-domain socket.
    public init(unixPath path: String) throws {
        fd = socket(AF_UNIX, SOCK_STREAM, 0)
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        if bytes.count >= MemoryLayout.size(ofValue: address.sun_path) {
            Foundation.close(fd)
            throw TrainerError("socket path too long: \(path)")
        }
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            for (i, byte) in bytes.enumerated() { raw[i] = byte }
        }
        let result = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        if result != 0 {
            Foundation.close(fd)
            throw TrainerError("cannot connect to \(path): \(String(cString: strerror(errno)))")
        }
    }

    deinit { Foundation.close(fd) }

    // MARK: Session

    /// Opens (or reopens) the session. `settings` are the optional keys of `hello`.
    public func hello(_ settings: [String: Any]) throws -> Description {
        var message = settings
        message["protocol"] = 1
        message["client"] = "flight-train"
        try send(.hello, Array(try JSONSerialization.data(withJSONObject: message)))
        let payload = try receive(expecting: .describe)
        guard let json = try JSONSerialization.jsonObject(with: Data(payload)) as? [String: Any] else {
            throw TrainerError("describe is not a JSON object")
        }
        let d = try Description(json: json)
        description = d
        return d
    }

    /// Starts every environment: environment `i` gets the seed `seed + i`. Returns `N * O` floats.
    /// With `stagger` the first episodes end at spread times (PROTOCOL.md, "reset").
    public func reset(seed: UInt64, stagger: Bool = false) throws -> [Float] {
        guard let d = description else { throw TrainerError("reset before hello") }
        try send(.reset, Array("{\"seed\":\(seed),\"stagger\":\(stagger)}".utf8))
        let payload = try receive(expecting: .observations)
        let n = d.numEnvs * d.observationLength
        if payload.count != 8 + 4 * n || Int(payload.u32(0)) != d.numEnvs || Int(payload.u32(4)) != d.observationLength {
            throw TrainerError("observations of the wrong shape")
        }
        return payload.floats(8, n)
    }

    /// Advances every environment one decision. `actions` is `N * A` floats.
    public func step(_ actions: [Float]) throws -> StepResult {
        guard let d = description else { throw TrainerError("step before hello") }
        let (envs, o) = (d.numEnvs, d.observationLength)
        precondition(actions.count == envs * d.actionLength)
        var payload = [UInt8](repeating: 0, count: 4 * actions.count)
        payload.withUnsafeMutableBytes { raw in
            actions.withUnsafeBytes { raw.copyMemory(from: $0) }  // little-endian on Apple silicon
        }
        try send(.step, payload)
        let r = try receive(expecting: .stepResult)
        let finished = Int(r.u32(8))
        let rewardsAt = 12 + 4 * envs * o
        let flagsAt = rewardsAt + 4 * envs
        var at = flagsAt + (envs + 3) / 4 * 4
        if Int(r.u32(0)) != envs || Int(r.u32(4)) != o || r.count != at + finished * (16 + 4 * o) {
            throw TrainerError("step result of the wrong size")
        }
        var result = StepResult(
            observations: r.floats(12, envs * o), rewards: r.floats(rewardsAt, envs),
            flags: Array(r[flagsAt..<flagsAt + envs]), ended: [])
        result.ended.reserveCapacity(finished)
        for _ in 0..<finished {
            result.ended.append(EpisodeEnd(
                env: Int(r.u32(at)), reason: Int(r.u32(at + 4)), episodeReturn: r.float(at + 8),
                length: Int(r.u32(at + 12)), lastObservation: r.floats(at + 16, o)))
            at += 16 + 4 * o
        }
        return result
    }

    public func close() {
        try? send(.close, [])
        _ = try? receive(expecting: .bye)
    }

    // MARK: Frames

    private func send(_ type: MessageType, _ payload: [UInt8]) throws {
        var frame = [UInt8]()
        frame.reserveCapacity(5 + payload.count)
        let length = UInt32(payload.count + 1)
        frame.append(contentsOf: [UInt8(length & 0xFF), UInt8(length >> 8 & 0xFF), UInt8(length >> 16 & 0xFF), UInt8(length >> 24)])
        frame.append(type.rawValue)
        frame.append(contentsOf: payload)
        var sent = 0
        while sent < frame.count {
            let n = frame.withUnsafeBytes { Foundation.send(fd, $0.baseAddress! + sent, frame.count - sent, 0) }
            if n <= 0 { throw TrainerError("the server closed the connection") }
            sent += n
        }
    }

    private func read(_ count: Int) throws -> [UInt8] {
        var buffer = [UInt8](repeating: 0, count: count)
        var got = 0
        while got < count {
            let n = buffer.withUnsafeMutableBytes { recv(fd, $0.baseAddress! + got, count - got, 0) }
            if n <= 0 { throw TrainerError("the server closed the connection") }
            got += n
        }
        return buffer
    }

    private func receive(expecting expected: MessageType) throws -> [UInt8] {
        let length = Int(try read(4).u32(0))
        if length < 1 { throw TrainerError("empty frame") }
        let body = try read(length)
        guard let type = MessageType(rawValue: body[0]) else { throw TrainerError("unknown message type \(body[0])") }
        let payload = Array(body.dropFirst())
        if type == .error { throw TrainerError("server error: \(String(decoding: payload, as: UTF8.self))") }
        if type != expected { throw TrainerError("expected \(expected), got \(type)") }
        return payload
    }
}

extension Array where Element == UInt8 {
    func u32(_ at: Int) -> UInt32 {
        withUnsafeBytes { UInt32(littleEndian: $0.loadUnaligned(fromByteOffset: at, as: UInt32.self)) }
    }

    func float(_ at: Int) -> Float { Float(bitPattern: u32(at)) }

    func floats(_ at: Int, _ count: Int) -> [Float] {
        [Float](unsafeUninitializedCapacity: count) { out, initialized in
            withUnsafeBytes { raw in
                UnsafeMutableRawBufferPointer(out).copyMemory(from: UnsafeRawBufferPointer(rebasing: raw[at..<at + 4 * count]))
            }
            initialized = count
        }
    }
}

// MARK: The server as a child process

/// A `flight-env` server started by the trainer on a Unix socket of its own; stopped with
/// `stop()` or when the trainer exits.
public final class ServerProcess {
    public let socketPath: String
    private let process = Process()

    /// Starts `binary` listening on a fresh socket and waits until it accepts connections.
    public init(binary: String, threads: Int?) throws {
        guard FileManager.default.isExecutableFile(atPath: binary) else {
            throw TrainerError("no flight-env server at \(binary): build it with `cargo build --release -p sim-env` or give --server-bin")
        }
        socketPath = "/tmp/flight-train-\(getpid()).sock"
        unlink(socketPath)
        process.executableURL = URL(fileURLWithPath: binary)
        var arguments = ["--unix", socketPath, "--once"]
        if let threads { arguments += ["--threads", String(threads)] }
        process.arguments = arguments
        process.standardOutput = FileHandle.nullDevice
        try process.run()
        for _ in 0..<200 {
            if FileManager.default.fileExists(atPath: socketPath) { return }
            if !process.isRunning { throw TrainerError("the flight-env server exited at start") }
            usleep(25_000)
        }
        throw TrainerError("the flight-env server did not open \(socketPath)")
    }

    public func stop() {
        if process.isRunning { process.terminate() }
        process.waitUntilExit()
        unlink(socketPath)
    }

    deinit { stop() }

    /// Where the server binary usually is: `CARGO_TARGET_DIR` or the repository's `target/`.
    public static func defaultBinary() -> String {
        if let dir = ProcessInfo.processInfo.environment["CARGO_TARGET_DIR"] {
            return dir + "/release/flight-env"
        }
        // trainer/ sits in the repository root; worktrees share the main repository's target.
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<4 { url.deleteLastPathComponent() }
        return url.appendingPathComponent("target/release/flight-env").path
    }
}
