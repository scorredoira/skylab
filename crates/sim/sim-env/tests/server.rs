//! The server, spoken to in raw bytes as PROTOCOL.md writes them.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use sim_env::json::Json;
use sim_env::observation;
use sim_env::server::{serve, Settings};
use sim_env::{Env, EnvConfig, Task};

/// A server on a free port, serving one client with the default settings.
fn connect() -> (TcpStream, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        serve(&mut stream, &Settings::default()).unwrap();
    });
    (TcpStream::connect(address).unwrap(), server)
}

fn receive(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut head = [0u8; 5];
    stream.read_exact(&mut head).unwrap();
    let length = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
    let mut payload = vec![0u8; length - 1];
    stream.read_exact(&mut payload).unwrap();
    (head[4], payload)
}

fn send(stream: &mut TcpStream, kind: u8, payload: &[u8]) {
    let mut frame = (payload.len() as u32 + 1).to_le_bytes().to_vec();
    frame.push(kind);
    frame.extend_from_slice(payload);
    stream.write_all(&frame).unwrap();
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn f32s(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn error(reply: (u8, Vec<u8>)) -> (String, bool) {
    assert_eq!(reply.0, 9, "not an error frame");
    let json = Json::parse(std::str::from_utf8(&reply.1).unwrap()).unwrap();
    (
        json.get("error").unwrap().as_str().unwrap().to_string(),
        json.get("fatal").unwrap().as_bool().unwrap(),
    )
}

#[test]
fn a_session_in_the_bytes_of_the_document() {
    let (mut s, server) = connect();
    let len = observation::len();

    // The annotated example of PROTOCOL.md, byte for byte.
    let hello = [
        0x2a, 0x00, 0x00, 0x00, 0x01, b'{', b'"', b'p', b'r', b'o', b't', b'o', b'c', b'o', b'l',
        b'"', b':', b'1', b',', b'"', b't', b'a', b's', b'k', b'"', b':', b'"', b'h', b'o', b'l',
        b'd', b'"', b',', b'"', b'n', b'u', b'm', b'_', b'e', b'n', b'v', b's', b'"', b':', b'2',
        b'}',
    ];
    s.write_all(&hello).unwrap();
    let (kind, payload) = receive(&mut s);
    assert_eq!(kind, 2);
    let describe = Json::parse(std::str::from_utf8(&payload).unwrap()).unwrap();
    assert_eq!(describe.get("protocol").unwrap().as_u64(), Some(1));
    assert_eq!(describe.get("num_envs").unwrap().as_u64(), Some(2));
    assert_eq!(describe.get("decision_dt").unwrap().as_f64(), Some(0.1));
    let o = describe.get("observation").unwrap();
    assert_eq!(o.get("len").unwrap().as_u64(), Some(len as u64));
    assert_eq!(o.get("image"), Some(&Json::Null));
    let fields = o.get("fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), len);
    for (f, ours) in fields.iter().zip(observation::FIELDS) {
        assert_eq!(f.get("name").unwrap().as_str(), Some(ours.name));
        assert_eq!(
            f.get("scale").unwrap().as_f64(),
            Some(f64::from(ours.scale))
        );
        assert!(!f.get("description").unwrap().as_str().unwrap().is_empty());
    }
    let a = describe.get("action").unwrap();
    assert_eq!(a.get("len").unwrap().as_u64(), Some(4));
    assert_eq!(a.get("mode").unwrap().as_str(), Some("incremental"));
    let names: Vec<_> = a
        .get("fields")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.get("name").unwrap().as_str().unwrap())
        .collect();
    assert_eq!(names, ["elevator", "aileron", "rudder", "throttle"]);
    let task = describe.get("task").unwrap();
    assert_eq!(task.get("name").unwrap().as_str(), Some("hold"));
    assert_eq!(task.get("max_steps").unwrap().as_u64(), Some(600));
    assert!(task
        .get("params")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p.get("name").unwrap().as_str() == Some("rewards.crash")));

    // reset: {"seed":7}
    s.write_all(&[
        0x0b, 0, 0, 0, 0x03, b'{', b'"', b's', b'e', b'e', b'd', b'"', b':', b'7', b'}',
    ])
    .unwrap();
    let (kind, payload) = receive(&mut s);
    assert_eq!(kind, 4);
    assert_eq!(payload.len(), 8 + 2 * len * 4);
    assert_eq!((u32_at(&payload, 0), u32_at(&payload, 4)), (2, len as u32));
    let first = f32s(&payload[8..]);

    // The same episodes in this process: seeds 7 and 8.
    let mut envs = [
        Env::new(EnvConfig::default()),
        Env::new(EnvConfig::default()),
    ];
    assert_eq!(envs[0].reset(7).observation, first[..len]);
    assert_eq!(envs[1].reset(8).observation, first[len..]);

    // step: environment 0 gets (0.5, -1, 0, 1), environment 1 gets zeros.
    let step = [
        0x21, 0x00, 0x00, 0x00, 0x05, //
        0x00, 0x00, 0x00, 0x3f, 0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80,
        0x3f, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    s.write_all(&step).unwrap();
    let (kind, payload) = receive(&mut s);
    assert_eq!(kind, 6);
    assert_eq!(payload.len(), 12 + 2 * len * 4 + 2 * 4 + 4);
    assert_eq!(
        (
            u32_at(&payload, 0),
            u32_at(&payload, 4),
            u32_at(&payload, 8)
        ),
        (2, len as u32, 0)
    );
    let numbers = f32s(&payload[12..12 + 2 * len * 4 + 8]);
    let expected = [
        envs[0].step(&[0.5, -1.0, 0.0, 1.0]),
        envs[1].step(&[0.0; 4]),
    ];
    assert_eq!(numbers[..len], expected[0].observation);
    assert_eq!(numbers[len..2 * len], expected[1].observation);
    assert_eq!(numbers[2 * len..], [expected[0].reward, expected[1].reward]);
    assert_eq!(payload[payload.len() - 4..], [0, 0, 0, 0]);

    // close
    s.write_all(&[0x01, 0, 0, 0, 0x07]).unwrap();
    assert_eq!(receive(&mut s), (8, vec![]));
    server.join().unwrap();
}

#[test]
fn finished_episodes_are_reported_with_their_last_observation() {
    let (mut s, server) = connect();
    let len = observation::len();
    // Three environments (so the flags need one byte of padding) and a 1 s time limit.
    send(&mut s, 1, br#"{"protocol":1,"task":"takeoff","num_envs":3,"threads":2,"action_mode":"direct","params":{"config.time_limit":1.0}}"#);
    let (kind, payload) = receive(&mut s);
    assert_eq!(kind, 2);
    let describe = Json::parse(std::str::from_utf8(&payload).unwrap()).unwrap();
    assert_eq!(
        describe
            .get("task")
            .unwrap()
            .get("max_steps")
            .unwrap()
            .as_u64(),
        Some(10)
    );
    assert_eq!(describe.get("threads").unwrap().as_u64(), Some(2));
    send(&mut s, 3, br#"{"seeds":[5,6,7]}"#);
    let (kind, first) = receive(&mut s);
    assert_eq!(kind, 4);

    let actions: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0]
        .repeat(3)
        .iter()
        .flat_map(|a| a.to_le_bytes())
        .collect();
    let mut last = Vec::new();
    for k in 0..10 {
        send(&mut s, 5, &actions);
        let (kind, payload) = receive(&mut s);
        assert_eq!(kind, 6);
        let finished = u32_at(&payload, 8) as usize;
        assert_eq!(finished, if k == 9 { 3 } else { 0 });
        assert_eq!(
            payload.len(),
            12 + 3 * len * 4 + 12 + 4 + finished * (16 + len * 4)
        );
        last = payload;
    }
    let flags_at = 12 + 3 * len * 4 + 12;
    assert_eq!(
        last[flags_at..flags_at + 4],
        [2, 2, 2, 0],
        "truncated, and one byte of padding"
    );
    let reasons = describe.get("reasons").unwrap().as_array().unwrap();
    let mut at = flags_at + 4;
    for env in 0..3 {
        assert_eq!(u32_at(&last, at), env);
        assert_eq!(
            reasons[u32_at(&last, at + 4) as usize].as_str(),
            Some("time_limit")
        );
        let episode_return = f32::from_le_bytes(last[at + 8..at + 12].try_into().unwrap());
        assert!(
            episode_return > 0.0,
            "full throttle gains speed: {episode_return}"
        );
        assert_eq!(u32_at(&last, at + 12), 10);
        let final_observation = f32s(&last[at + 16..at + 16 + len * 4]);
        let next = f32s(&last[12 + env as usize * len * 4..12 + (env as usize + 1) * len * 4]);
        // Full throttle and moving at the end of the old episode, idle and standing at the start of the new one.
        let (speed, throttle) = (
            observation::index("ground_speed").unwrap(),
            observation::index("throttle").unwrap(),
        );
        assert!(
            final_observation[speed] > next[speed] + 0.002,
            "{} {}",
            final_observation[speed],
            next[speed]
        );
        assert_eq!((final_observation[throttle], next[throttle]), (1.0, 0.0));
        at += 16 + len * 4;
    }
    assert_eq!(at, last.len());
    assert_eq!(first.len(), 8 + 3 * len * 4);

    // Staggered: the three first episodes end after 3, 6 and 10 decisions.
    assert_eq!(describe.get("stagger").and_then(Json::as_bool), Some(true));
    send(&mut s, 3, br#"{"seed":5,"stagger":true}"#);
    assert_eq!(receive(&mut s).0, 4);
    let mut ended = Vec::new();
    for k in 1..=10 {
        send(&mut s, 5, &actions);
        let (_, payload) = receive(&mut s);
        for e in 0..u32_at(&payload, 8) as usize {
            let at = flags_at + 4 + e * (16 + len * 4);
            ended.push((k, u32_at(&payload, at), u32_at(&payload, at + 12)));
        }
    }
    assert_eq!(ended, [(3, 0, 3), (6, 1, 6), (10, 2, 10)]);
    drop(s); // leaving without a close is fine too
    server.join().unwrap();
}

#[test]
fn mistakes_get_an_error_and_the_session_goes_on() {
    let (mut s, server) = connect();
    send(&mut s, 5, &[0; 16]);
    assert_eq!(
        error(receive(&mut s)),
        ("say hello first".to_string(), false)
    );
    send(&mut s, 1, br#"{"protocol":1,"task":"gliding"}"#);
    let (message, fatal) = error(receive(&mut s));
    assert!(
        message.contains("unknown task gliding") && !fatal,
        "{message}"
    );
    send(
        &mut s,
        1,
        br#"{"protocol":1,"params":{"rewards.nothing":1}}"#,
    );
    assert!(error(receive(&mut s))
        .0
        .contains("no parameter rewards.nothing"));
    send(&mut s, 1, b"{not json");
    assert!(error(receive(&mut s)).0.starts_with("JSON"));
    send(&mut s, 1, br#"{"protocol":1,"num_envs":2}"#);
    assert_eq!(receive(&mut s).0, 2);
    send(&mut s, 5, &[0; 32]);
    assert_eq!(error(receive(&mut s)).0, "reset before the first step");
    send(&mut s, 3, br#"{"seeds":[1]}"#);
    assert_eq!(error(receive(&mut s)).0, "seeds must be 2 whole numbers");
    send(&mut s, 3, br#"{"seed":1}"#);
    assert_eq!(receive(&mut s).0, 4);
    send(&mut s, 5, &[0; 12]);
    let (message, fatal) = error(receive(&mut s));
    assert!(
        message.contains("12 bytes") && message.contains("need 32") && !fatal,
        "{message}"
    );
    send(&mut s, 42, &[]);
    assert_eq!(error(receive(&mut s)).0, "unknown message type 42");
    // Still alive; a second hello starts a new session.
    send(&mut s, 5, &[0; 32]);
    assert_eq!(receive(&mut s).0, 6);
    send(
        &mut s,
        1,
        br#"{"protocol":1,"task":"landing","num_envs":1,"flaps":true,"trim":true}"#,
    );
    let (kind, payload) = receive(&mut s);
    assert_eq!(kind, 2);
    let describe = Json::parse(std::str::from_utf8(&payload).unwrap()).unwrap();
    assert_eq!(
        describe.get("action").unwrap().get("len").unwrap().as_u64(),
        Some(6)
    );
    // Another protocol version, and a frame too large to be one, end the connection.
    send(&mut s, 1, br#"{"protocol":2}"#);
    let (message, fatal) = error(receive(&mut s));
    assert!(message.contains("protocol 1") && fatal, "{message}");
    server.join().unwrap();

    let (mut s, server) = connect();
    s.write_all(&[0xff, 0xff, 0xff, 0xff, 1]).unwrap();
    assert!(error(receive(&mut s)).1);
    server.join().unwrap();
    // Every task can be asked for by name.
    assert!(Task::NAMES.iter().all(|n| Task::parse(n).is_some()));
}

#[cfg(unix)]
#[test]
fn the_same_over_a_unix_socket() {
    use std::os::unix::net::{UnixListener, UnixStream};
    let path = std::env::temp_dir().join(format!("sim-env-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        serve(&mut stream, &Settings::default()).unwrap();
    });
    let mut s = UnixStream::connect(&path).unwrap();
    s.write_all(&[15, 0, 0, 0, 1]).unwrap();
    s.write_all(br#"{"protocol":1}"#).unwrap();
    let mut head = [0u8; 5];
    s.read_exact(&mut head).unwrap();
    assert_eq!(head[4], 2);
    drop(s);
    server.join().unwrap();
    std::fs::remove_file(&path).unwrap();
}
