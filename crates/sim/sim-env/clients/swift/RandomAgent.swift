// A minimal client of the sim-env server, written from PROTOCOL.md alone: it connects, reads
// the description, plays random actions and prints the episode returns and the speed.
// Foundation only, no packages.
//
//   swiftc -O RandomAgent.swift -o random-agent
//   ./random-agent [--port 7447 | --unix PATH] [--task NAME] [--envs N] [--steps N] [--seed N]

import Foundation

#if canImport(Glibc)
import Glibc
let streamSocket = Int32(SOCK_STREAM.rawValue)
#else
let streamSocket = SOCK_STREAM
#endif

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("random-agent: \(message)\n".utf8))
    exit(1)
}

// MARK: Messages

enum Message: UInt8 {
    case hello = 1, describe, reset, observations, step, stepResult, close, bye, error
}

/// A connection to the server: frames of `u32 length | u8 type | payload`, little-endian.
struct Connection {
    let fd: Int32

    init(port: UInt16) {
        fd = socket(AF_INET, streamSocket, 0)
        var address = sockaddr_in()
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = port.bigEndian
        address.sin_addr.s_addr = inet_addr("127.0.0.1")
        let result = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        if result != 0 { fail("cannot connect to 127.0.0.1:\(port): \(String(cString: strerror(errno)))") }
        // Send small frames at once.
        var one: Int32 = 1
        setsockopt(fd, Int32(IPPROTO_TCP), TCP_NODELAY, &one, socklen_t(MemoryLayout<Int32>.size))
    }

    init(unixPath path: String) {
        fd = socket(AF_UNIX, streamSocket, 0)
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        if bytes.count >= MemoryLayout.size(ofValue: address.sun_path) { fail("socket path too long") }
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            for (i, byte) in bytes.enumerated() { raw[i] = byte }
        }
        let result = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        if result != 0 { fail("cannot connect to \(path): \(String(cString: strerror(errno)))") }
    }

    func send(_ type: Message, _ payload: [UInt8] = []) {
        var frame = [UInt8]()
        frame.reserveCapacity(5 + payload.count)
        appendUInt32(&frame, UInt32(payload.count + 1))
        frame.append(type.rawValue)
        frame.append(contentsOf: payload)
        var sent = 0
        while sent < frame.count {
            let n = frame.withUnsafeBytes { Foundation.send(fd, $0.baseAddress! + sent, frame.count - sent, 0) }
            if n <= 0 { fail("the server closed the connection") }
            sent += n
        }
    }

    private func read(_ count: Int) -> [UInt8] {
        var buffer = [UInt8](repeating: 0, count: count)
        var got = 0
        while got < count {
            let n = buffer.withUnsafeMutableBytes { recv(fd, $0.baseAddress! + got, count - got, 0) }
            if n <= 0 { fail("the server closed the connection") }
            got += n
        }
        return buffer
    }

    /// The next frame. An error frame ends the program with the server's message.
    func receive(expecting expected: Message) -> [UInt8] {
        let length = Int(readUInt32(read(4), 0))
        let body = read(length)
        guard let type = Message(rawValue: body[0]) else { fail("unknown message type \(body[0])") }
        let payload = Array(body.dropFirst())
        if type == .error { fail("server error: \(String(decoding: payload, as: UTF8.self))") }
        if type != expected { fail("expected \(expected), got \(type)") }
        return payload
    }
}

// MARK: Little-endian numbers

func appendUInt32(_ out: inout [UInt8], _ value: UInt32) {
    out.append(UInt8(value & 0xFF))
    out.append(UInt8((value >> 8) & 0xFF))
    out.append(UInt8((value >> 16) & 0xFF))
    out.append(UInt8((value >> 24) & 0xFF))
}

func readUInt32(_ bytes: [UInt8], _ offset: Int) -> UInt32 {
    UInt32(bytes[offset]) | UInt32(bytes[offset + 1]) << 8 | UInt32(bytes[offset + 2]) << 16
        | UInt32(bytes[offset + 3]) << 24
}

func readFloat(_ bytes: [UInt8], _ offset: Int) -> Float {
    Float(bitPattern: readUInt32(bytes, offset))
}

func readFloats(_ bytes: [UInt8], _ offset: Int, _ count: Int) -> [Float] {
    (0..<count).map { readFloat(bytes, offset + 4 * $0) }
}

func json(_ object: [String: Any]) -> [UInt8] {
    Array(try! JSONSerialization.data(withJSONObject: object))
}

// MARK: Arguments

func number(_ text: String, _ flag: String) -> Int {
    guard let n = Int(text), n >= 0, n <= 1 << 40 else { fail("bad value for \(flag)") }
    return n
}

var port: UInt16 = 7447
var unixPath: String?
var hello: [String: Any] = ["protocol": 1, "client": "RandomAgent.swift"]
var totalSteps = 5000
var seed = 1
var arguments = CommandLine.arguments.dropFirst().makeIterator()
while let flag = arguments.next() {
    guard let value = arguments.next() else { fail("\(flag) needs a value") }
    switch flag {
    case "--port": port = UInt16(clamping: number(value, flag))
    case "--unix": unixPath = value
    case "--task": hello["task"] = value
    case "--envs": hello["num_envs"] = number(value, flag)
    case "--steps": totalSteps = number(value, flag)
    case "--seed": seed = number(value, flag)
    default: fail("unknown option \(flag)")
    }
}

// MARK: Hello and describe

let connection = unixPath.map { Connection(unixPath: $0) } ?? Connection(port: port)
connection.send(.hello, json(hello))
let describeBytes = connection.receive(expecting: .describe)
guard let description = try? JSONSerialization.jsonObject(with: Data(describeBytes)) as? [String: Any],
    let observation = description["observation"] as? [String: Any],
    let action = description["action"] as? [String: Any],
    let task = description["task"] as? [String: Any],
    let reasons = description["reasons"] as? [String],
    let envs = description["num_envs"] as? Int,
    let observationLength = observation["len"] as? Int,
    let actionLength = action["len"] as? Int,
    let decisionDt = description["decision_dt"] as? Double
else { fail("cannot read the description") }

let observationNames = (observation["fields"] as? [[String: Any]] ?? []).compactMap { $0["name"] as? String }
let actionNames = (action["fields"] as? [[String: Any]] ?? []).compactMap { $0["name"] as? String }
print("server: \(description["server"] ?? "?"), protocol \(description["protocol"] ?? "?")")
print("task: \(task["name"] ?? "?"), time limit \(task["time_limit"] ?? "?") s (\(task["max_steps"] ?? "?") steps), \(envs) environments on \(description["threads"] ?? "?") threads, a decision every \(decisionDt) s")
print("observation (\(observationLength)): \(observationNames.joined(separator: " "))")
print("action (\(actionLength), \(action["mode"] ?? "?")): \(actionNames.joined(separator: " "))")

// MARK: Reset

connection.send(.reset, json(["seed": seed]))
let first = connection.receive(expecting: .observations)
if Int(readUInt32(first, 0)) != envs || Int(readUInt32(first, 4)) != observationLength
    || first.count != 8 + 4 * envs * observationLength
{
    fail("the first observations do not have the described shape")
}
let firstObservation = readFloats(first, 8, observationLength)
print("first observation of environment 0: " + firstObservation.map { String(format: "%.2f", $0) }.joined(separator: " "))

// MARK: Random steps

var generator = SystemRandomNumberGenerator()
var actionBytes = [UInt8]()
var episodes = 0
var returnSum = 0.0
var lengthSum = 0
var rewardSum = 0.0
var reasonCounts = [Int](repeating: 0, count: reasons.count)
var steps = 0
let started = Date()
while steps < totalSteps {
    actionBytes.removeAll(keepingCapacity: true)
    for _ in 0..<(envs * actionLength) {
        appendUInt32(&actionBytes, Float.random(in: -1...1, using: &generator).bitPattern)
    }
    connection.send(.step, actionBytes)
    let result = connection.receive(expecting: .stepResult)

    // u32 envs | u32 observation length | u32 finished | observations | rewards | flags (padded to 4) | finished records
    let finished = Int(readUInt32(result, 8))
    let rewardsAt = 12 + 4 * envs * observationLength
    let flagsAt = rewardsAt + 4 * envs
    var at = flagsAt + (envs + 3) / 4 * 4
    if result.count != at + finished * (16 + 4 * observationLength) { fail("step result of the wrong size") }
    for env in 0..<envs { rewardSum += Double(readFloat(result, rewardsAt + 4 * env)) }
    var flagged = 0
    for env in 0..<envs where result[flagsAt + env] != 0 { flagged += 1 }
    if flagged != finished { fail("flags and finished episodes disagree") }
    for _ in 0..<finished {
        let env = Int(readUInt32(result, at))
        let reason = Int(readUInt32(result, at + 4))
        let episodeReturn = readFloat(result, at + 8)
        let length = Int(readUInt32(result, at + 12))
        at += 16 + 4 * observationLength
        if episodes < 5 {
            print(String(format: "episode %d: environment %d, return %.2f, %d steps (%.1f s), %@",
                episodes, env, episodeReturn, length, Double(length) * decisionDt, reasons[reason]))
        }
        episodes += 1
        returnSum += Double(episodeReturn)
        lengthSum += length
        reasonCounts[reason] += 1
    }
    steps += envs
}
let elapsed = Date().timeIntervalSince(started)

connection.send(.close)
_ = connection.receive(expecting: .bye)

let ends = zip(reasons, reasonCounts).filter { $0.1 > 0 }.map { "\($0.0) \($0.1)" }.joined(separator: ", ")
print("...")
print(String(format: "%d environment steps, %d episodes finished: mean return %.2f, mean length %.1f steps [%@]",
    steps, episodes, returnSum / Double(max(episodes, 1)), Double(lengthSum) / Double(max(episodes, 1)), ends))
print(String(format: "mean reward per step %.4f", rewardSum / Double(steps)))
print(String(format: "%.0f environment steps per second, %.0f simulated seconds per second (%.2f s)",
    Double(steps) / elapsed, Double(steps) * decisionDt / elapsed, elapsed))
