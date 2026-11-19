//! The server: a `VecEnv` for one client over a byte stream. The protocol is `PROTOCOL.md`.

use std::io::{self, Read, Write};

use sim_core::{AircraftKind, MultirotorKind, World};

use crate::action::ActionMode;
use crate::env::EnvConfig;
use crate::json::Json;
use crate::observation;
use crate::task::{Reason, Task};
use crate::vec_env::{performance_cores, VecEnv};

/// Version of the protocol.
pub const PROTOCOL: u64 = 1;
/// Largest frame accepted, bytes.
pub const MAX_FRAME: usize = 64 << 20;
/// Most environments a session may ask for.
pub const MAX_ENVS: usize = 65_536;

/// Message types: the byte after the length.
pub mod message {
    pub const HELLO: u8 = 1;
    pub const DESCRIBE: u8 = 2;
    pub const RESET: u8 = 3;
    pub const OBSERVATIONS: u8 = 4;
    pub const STEP: u8 = 5;
    pub const STEP_RESULT: u8 = 6;
    pub const CLOSE: u8 = 7;
    pub const BYE: u8 = 8;
    pub const ERROR: u8 = 9;
}

/// What a session runs: the server's flags give the defaults and `hello` can change them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub env: EnvConfig,
    pub num_envs: usize,
    /// Worker threads; 0 means one per performance core.
    pub threads: usize,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            env: EnvConfig::default(),
            num_envs: 64,
            threads: 0,
        }
    }
}

impl Settings {
    pub fn threads(&self) -> usize {
        let wanted = if self.threads == 0 {
            performance_cores()
        } else {
            self.threads
        };
        wanted.clamp(1, self.num_envs.max(1))
    }

    /// Changes one setting by the name it has in `hello` (and, with dashes, on the command line).
    /// `task` replaces every task number with that task's defaults, so give it first.
    pub fn set(&mut self, key: &str, value: &Json) -> Result<(), String> {
        let wrong = || format!("bad value for {key}: {value}");
        let whole = || value.as_u64().ok_or_else(wrong);
        match key {
            "task" => {
                let name = value.as_str().ok_or_else(wrong)?;
                self.env.task = Task::parse(name)
                    .ok_or_else(|| format!("unknown task {name}: one of {:?}", Task::NAMES))?;
            }
            "num_envs" => {
                let n = whole()? as usize;
                if n == 0 || n > MAX_ENVS {
                    return Err(format!("num_envs must be between 1 and {MAX_ENVS}"));
                }
                self.num_envs = n;
            }
            "threads" => self.threads = (whole()? as usize).min(1024),
            "world_seed" => self.env.world_seed = whole()?,
            "aircraft" => {
                let name = value.as_str().ok_or_else(wrong)?;
                self.env.aircraft = AircraftKind::parse(name)
                    .ok_or_else(|| format!("unknown aircraft {name}: c172 or extra"))?;
            }
            "multirotor" => {
                let name = value.as_str().ok_or_else(wrong)?;
                self.env.multirotor = MultirotorKind::parse(name)
                    .ok_or_else(|| format!("unknown multirotor {name}: fpv5"))?;
            }
            "decision_dt" => self.env.decision_dt = value.as_f64().ok_or_else(wrong)?,
            "action_mode" => {
                self.env.action.mode = value
                    .as_str()
                    .and_then(ActionMode::parse)
                    .ok_or_else(wrong)?
            }
            "flaps" => self.env.action.flaps = value.as_bool().ok_or_else(wrong)?,
            "trim" => self.env.action.trim = value.as_bool().ok_or_else(wrong)?,
            "params" => {
                for (name, v) in value.as_object().ok_or_else(wrong)? {
                    let x = v
                        .as_f64()
                        .ok_or_else(|| format!("bad value for {name}: {v}"))?;
                    if !self.env.task.set(name, x) {
                        return Err(format!(
                            "the task {} has no parameter {name}",
                            self.env.task.name()
                        ));
                    }
                }
            }
            "protocol" | "client" => {}
            _ => return Err(format!("unknown setting {key}")),
        }
        Ok(())
    }

    /// Applies a `hello`: `task` first, then the rest in the order written.
    pub fn apply(&mut self, hello: &Json) -> Result<(), String> {
        let pairs = hello.as_object().ok_or("hello must be a JSON object")?;
        if let Some(task) = hello.get("task") {
            self.set("task", task)?;
        }
        for (key, value) in pairs.iter().filter(|(k, _)| k != "task") {
            self.set(key, value)?;
        }
        Ok(())
    }

    /// The `describe` message for these settings.
    pub fn describe(&self) -> Json {
        let env = &self.env;
        let fields = observation::fields(&env.task)
            .iter()
            .map(|f| {
                Json::obj(vec![
                    ("name", Json::str(f.name)),
                    ("scale", Json::num(f.scale)),
                    ("description", Json::str(f.description)),
                ])
            })
            .collect();
        let actions = env
            .action
            .fields_for(&env.task)
            .iter()
            .map(|f| {
                Json::obj(vec![
                    ("name", Json::str(f.name)),
                    (
                        "description",
                        Json::str(match env.action.mode {
                            ActionMode::Direct => f.direct,
                            ActionMode::Incremental => f.incremental,
                        }),
                    ),
                ])
            })
            .collect();
        let mut params = Vec::new();
        for (section, list) in env.task.params() {
            for p in list {
                params.push(Json::obj(vec![
                    ("name", Json::str(format!("{section}.{}", p.name))),
                    ("value", Json::Num(p.value)),
                    ("description", Json::str(p.description)),
                ]));
            }
        }
        let substeps = env.substeps();
        let dt = substeps as f64 * World::DT;
        Json::obj(vec![
            ("protocol", Json::num(PROTOCOL as f64)),
            (
                "server",
                Json::str(concat!("flight-env ", env!("CARGO_PKG_VERSION"))),
            ),
            ("num_envs", Json::num(self.num_envs as f64)),
            ("threads", Json::num(self.threads() as f64)),
            ("world_seed", Json::num(env.world_seed as f64)),
            ("aircraft", Json::str(env.aircraft.key())),
            ("multirotor", Json::str(env.multirotor.key())),
            (
                "vehicle",
                Json::str(if env.task.car() { env.car.key() } else if env.task.multirotor() { env.multirotor.key() } else { env.aircraft.key() }),
            ),
            ("decision_dt", Json::Num(dt)),
            ("substeps", Json::num(substeps as f64)),
            (
                "observation",
                Json::obj(vec![
                    ("len", Json::num(env.observation_len() as f64)),
                    ("clip", Json::num(observation::CLIP)),
                    ("fields", Json::Arr(fields)),
                    ("image", Json::Null),
                ]),
            ),
            (
                "action",
                Json::obj(vec![
                    ("len", Json::num(env.action_len() as f64)),
                    ("mode", Json::str(env.action.mode.name())),
                    ("low", Json::num(-1.0)),
                    ("high", Json::num(1.0)),
                    ("fields", Json::Arr(actions)),
                ]),
            ),
            (
                "task",
                Json::obj(vec![
                    ("name", Json::str(env.task.name())),
                    ("time_limit", Json::Num(env.task.time_limit())),
                    (
                        "max_steps",
                        Json::num((env.task.time_limit() / dt - 1e-6).ceil()),
                    ),
                    ("params", Json::Arr(params)),
                ]),
            ),
            (
                "tasks",
                Json::Arr(Task::NAMES.iter().map(|n| Json::str(*n)).collect()),
            ),
            (
                "reasons",
                Json::Arr(Reason::NAMES.iter().map(|n| Json::str(*n)).collect()),
            ),
            // `reset` understands `stagger`.
            ("stagger", Json::Bool(true)),
        ])
    }
}

/// Reads one frame: `(type, payload)`. `None` when the stream ended cleanly between frames.
pub fn read_frame(stream: &mut impl Read) -> io::Result<Option<(u8, Vec<u8>)>> {
    let mut head = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        match stream.read(&mut head[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let len = u32::from_le_bytes(head) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {len} bytes"),
        ));
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body)?;
    let kind = body.remove(0);
    Ok(Some((kind, body)))
}

/// Writes one frame.
pub fn write_frame(stream: &mut impl Write, kind: u8, payload: &[u8]) -> io::Result<()> {
    let mut out = Vec::with_capacity(5 + payload.len());
    out.extend_from_slice(&(payload.len() as u32 + 1).to_le_bytes());
    out.push(kind);
    out.extend_from_slice(payload);
    stream.write_all(&out)?;
    stream.flush()
}

fn put_f32(out: &mut Vec<u8>, values: &[f32]) {
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn send_error(stream: &mut impl Write, message: &str, fatal: bool) -> io::Result<()> {
    let body = Json::obj(vec![
        ("error", Json::str(message)),
        ("fatal", Json::Bool(fatal)),
    ]);
    write_frame(stream, message::ERROR, body.to_string().as_bytes())
}

/// Serves one client until it closes. `defaults` is what a `hello` without settings gets.
pub fn serve<S: Read + Write>(stream: &mut S, defaults: &Settings) -> io::Result<()> {
    let mut session: Option<(Settings, VecEnv)> = None;
    let mut actions: Vec<f32> = Vec::new();
    let mut out: Vec<u8> = Vec::new();
    loop {
        let (kind, payload) = match read_frame(stream) {
            Ok(Some(frame)) => frame,
            Ok(None) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                // The framing is lost: nothing more can be read from this stream.
                send_error(stream, &e.to_string(), true)?;
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        match kind {
            message::HELLO => {
                let parsed = std::str::from_utf8(&payload)
                    .map_err(|_| "hello is not UTF-8".to_string())
                    .and_then(Json::parse);
                let hello = match parsed {
                    Ok(h) => h,
                    Err(e) => {
                        send_error(stream, &e, false)?;
                        continue;
                    }
                };
                if hello.get("protocol").and_then(Json::as_u64) != Some(PROTOCOL) {
                    send_error(
                        stream,
                        &format!("this server speaks protocol {PROTOCOL}; hello must say \"protocol\":{PROTOCOL}"),
                        true,
                    )?;
                    return Ok(());
                }
                let mut settings = *defaults;
                let checked = settings.apply(&hello).and_then(|()| {
                    settings
                        .env
                        .validate(&settings.env.terrain())
                });
                if let Err(e) = checked {
                    send_error(stream, &e, false)?;
                    continue;
                }
                // The old environments stop before the new ones start.
                drop(session.take());
                let envs = VecEnv::new(settings.env, settings.num_envs, settings.threads());
                write_frame(
                    stream,
                    message::DESCRIBE,
                    settings.describe().to_string().as_bytes(),
                )?;
                session = Some((settings, envs));
            }
            message::RESET => {
                let Some((settings, envs)) = session.as_mut() else {
                    send_error(stream, "say hello first", false)?;
                    continue;
                };
                let n = settings.num_envs;
                let request = std::str::from_utf8(&payload)
                    .map_err(|_| "reset is not UTF-8".to_string())
                    .and_then(Json::parse)
                    .and_then(|r| {
                        if let Some(list) = r.get("seeds").and_then(Json::as_array) {
                            let seeds: Option<Vec<u64>> = list.iter().map(Json::as_u64).collect();
                            seeds
                                .filter(|s| s.len() == n)
                                .ok_or_else(|| format!("seeds must be {n} whole numbers"))
                        } else if let Some(seed) = r.get("seed").and_then(Json::as_u64) {
                            Ok((0..n as u64).map(|i| seed.wrapping_add(i)).collect())
                        } else {
                            Err("reset needs \"seed\" or \"seeds\"".to_string())
                        }
                    });
                let stagger = std::str::from_utf8(&payload)
                    .ok()
                    .and_then(|t| Json::parse(t).ok())
                    .and_then(|r| r.get("stagger").and_then(Json::as_bool))
                    .unwrap_or(false);
                let seeds = match request {
                    Ok(s) => s,
                    Err(e) => {
                        send_error(stream, &e, false)?;
                        continue;
                    }
                };
                let observations = if stagger {
                    envs.reset_staggered(&seeds)
                } else {
                    envs.reset(&seeds)
                };
                out.clear();
                put_u32(&mut out, n as u32);
                put_u32(&mut out, settings.env.observation_len() as u32);
                put_f32(&mut out, observations);
                write_frame(stream, message::OBSERVATIONS, &out)?;
            }
            message::STEP => {
                let Some((settings, envs)) = session.as_mut() else {
                    send_error(stream, "say hello first", false)?;
                    continue;
                };
                if !envs.started() {
                    send_error(stream, "reset before the first step", false)?;
                    continue;
                }
                let n = settings.num_envs;
                let expected = n * envs.action_len() * 4;
                if payload.len() != expected {
                    send_error(
                        stream,
                        &format!(
                            "step carries {} bytes; {n} environments of {} actions need {expected}",
                            payload.len(),
                            envs.action_len()
                        ),
                        false,
                    )?;
                    continue;
                }
                actions.clear();
                actions.extend(
                    payload
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|c| f32::from_le_bytes(*c)),
                );
                let step = envs.step(&actions);
                out.clear();
                put_u32(&mut out, n as u32);
                put_u32(&mut out, settings.env.observation_len() as u32);
                put_u32(&mut out, step.finished.len() as u32);
                put_f32(&mut out, &step.observations);
                put_f32(&mut out, &step.rewards);
                out.extend_from_slice(&step.flags);
                out.resize(out.len() + (4 - n % 4) % 4, 0);
                for f in &step.finished {
                    put_u32(&mut out, f.env);
                    put_u32(&mut out, f.reason as u32);
                    put_f32(&mut out, &[f.episode_return]);
                    put_u32(&mut out, f.steps);
                    put_f32(&mut out, &f.final_observation);
                }
                write_frame(stream, message::STEP_RESULT, &out)?;
            }
            message::CLOSE => {
                write_frame(stream, message::BYE, &[])?;
                return Ok(());
            }
            other => send_error(stream, &format!("unknown message type {other}"), false)?,
        }
    }
}
