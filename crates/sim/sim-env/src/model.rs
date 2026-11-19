//! A trained policy flown inside the simulator: a small multilayer perceptron read from a file,
//! and `ModelPilot`, the `Pilot` that flies with it. No machine-learning dependency: the forward
//! pass is a few loops.
//!
//! The file format is in `PROTOCOL.md` ("Model file"). `ModelPilot` builds its observation with
//! the same code as the environment (`observation::write`, `Guidance`) and moves the controls
//! with the same `ActionMapper`, at the same decision rate, so a model flies in the game exactly
//! as it flew in training.

use std::path::Path;

use std::sync::Arc;

use sim_core::car::CarType;
use sim_core::circuit::Circuit;
use sim_core::{AircraftControls, CarControls, Controls, MultirotorControls, Observation, Pilot, World};

use crate::action::{ActionConfig, ActionMapper, ActionMode};
use crate::guidance::{Guidance, Targets};
use crate::json::Json;
use crate::observation::{self, Field, Inputs};
use crate::task::Task;

/// First eight bytes of a model file.
pub const MAGIC: &[u8; 8] = b"FLTMODEL";
/// Version of the model file format.
pub const FORMAT: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    Tanh,
    Relu,
}

impl Activation {
    pub fn name(self) -> &'static str {
        match self {
            Activation::Tanh => "tanh",
            Activation::Relu => "relu",
        }
    }

    fn apply(self, x: f32) -> f32 {
        match self {
            Activation::Tanh => x.tanh(),
            Activation::Relu => x.max(0.0),
        }
    }
}

/// How the last layer's output becomes an action in [-1, 1].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Squash {
    Tanh,
    /// Clamp to [-1, 1].
    Clip,
}

impl Squash {
    pub fn name(self) -> &'static str {
        match self {
            Squash::Tanh => "tanh",
            Squash::Clip => "clip",
        }
    }
}

/// One fully connected layer: `y = W x + b`.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub inputs: usize,
    pub outputs: usize,
    /// `outputs * inputs` values, row by row: `weights[o * inputs + i]`.
    pub weights: Vec<f32>,
    pub bias: Vec<f32>,
}

impl Layer {
    pub fn zeros(inputs: usize, outputs: usize) -> Layer {
        Layer {
            inputs,
            outputs,
            weights: vec![0.0; inputs * outputs],
            bias: vec![0.0; outputs],
        }
    }

    /// The weight from input `i` to output `o`.
    pub fn weight_mut(&mut self, o: usize, i: usize) -> &mut f32 {
        &mut self.weights[o * self.inputs + i]
    }
}

/// A policy: what it was trained on and its network. The action is the mean one (deterministic).
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    /// Task name as in the protocol (`hold`, `takeoff`, `landing`, `circuit`, `race`).
    pub task: String,
    /// Names of the observation fields it was trained on, in order.
    pub observation: Vec<String>,
    pub action: ActionConfig,
    /// Seconds between decisions.
    pub decision_dt: f64,
    pub activation: Activation,
    pub squash: Squash,
    /// Observation normalisation: the input is `(obs - mean) / std`.
    pub normalization: Option<(Vec<f32>, Vec<f32>)>,
    pub layers: Vec<Layer>,
}

fn bad(what: impl Into<String>) -> String {
    format!("model file: {}", what.into())
}

impl Model {
    /// An untrained model for the current observation layout: every weight zero.
    pub fn new(
        task: &str,
        action: ActionConfig,
        hidden: &[usize],
        activation: Activation,
    ) -> Model {
        let mut sizes = vec![observation::len()];
        sizes.extend_from_slice(hidden);
        sizes.push(action.len());
        Model {
            task: task.to_string(),
            observation: observation::FIELDS
                .iter()
                .map(|f| f.name.to_string())
                .collect(),
            action,
            decision_dt: 0.1,
            activation,
            squash: Squash::Tanh,
            normalization: None,
            layers: sizes.windows(2).map(|s| Layer::zeros(s[0], s[1])).collect(),
        }
    }

    /// Whether the shapes fit together.
    pub fn validate(&self) -> Result<(), String> {
        let n = self.observation.len();
        if self.layers.is_empty() {
            return Err(bad("no layers"));
        }
        let mut width = n;
        for (k, l) in self.layers.iter().enumerate() {
            if l.inputs != width || l.inputs == 0 || l.outputs == 0 {
                return Err(bad(format!(
                    "layer {k} takes {} inputs but gets {width}",
                    l.inputs
                )));
            }
            if l.weights.len() != l.inputs * l.outputs || l.bias.len() != l.outputs {
                return Err(bad(format!("layer {k} has the wrong number of weights")));
            }
            width = l.outputs;
        }
        if width != self.action_len() {
            return Err(bad(format!(
                "the network gives {width} outputs but there are {} actions",
                self.action_len()
            )));
        }
        if let Some((mean, std)) = &self.normalization {
            if mean.len() != n || std.len() != n {
                return Err(bad("normalisation of the wrong length"));
            }
        }
        if !(self.decision_dt.is_finite() && self.decision_dt > 0.0) {
            return Err(bad("bad decision_dt"));
        }
        Ok(())
    }

    /// The observation table of its task (`observation::fields`): the aircraft's, the
    /// multirotor's for `hover`, with the gates for `gates`.
    pub fn fields(&self) -> &'static [Field] {
        Task::parse(&self.task).map_or(observation::FIELDS, |t| observation::fields(&t))
    }

    /// Whether it flies a multirotor (`hover`, `gates`) rather than an aircraft.
    pub fn multirotor(&self) -> bool {
        Task::parse(&self.task).is_some_and(|t| t.multirotor())
    }

    /// Whether it was trained on the observation layout this build produces for its task.
    pub fn check_layout(&self) -> Result<(), String> {
        let ours: Vec<&str> = self.fields().iter().map(|f| f.name).collect();
        if self
            .observation
            .iter()
            .map(String::as_str)
            .ne(ours.iter().copied())
        {
            let at = (0..ours.len().max(self.observation.len()))
                .find(|&i| self.observation.get(i).map(String::as_str) != ours.get(i).copied())
                .unwrap_or(0);
            return Err(bad(format!(
                "trained on another observation layout: field {at} is {:?} in the file and {:?} here",
                self.observation.get(at).map_or("nothing", String::as_str),
                ours.get(at).copied().unwrap_or("nothing"),
            )));
        }
        Ok(())
    }

    /// The action for an observation. `scratch` is working memory, kept between calls.
    pub fn act(&self, obs: &[f32], scratch: &mut Scratch, out: &mut [f32]) {
        assert_eq!(obs.len(), self.observation.len());
        assert_eq!(out.len(), self.action_len());
        let (a, b) = (&mut scratch.a, &mut scratch.b);
        a.clear();
        match &self.normalization {
            Some((mean, std)) => a.extend(
                obs.iter()
                    .zip(mean.iter().zip(std))
                    .map(|(x, (m, s))| (x - m) / s.max(1e-6)),
            ),
            None => a.extend_from_slice(obs),
        }
        let last = self.layers.len() - 1;
        for (k, l) in self.layers.iter().enumerate() {
            b.clear();
            for o in 0..l.outputs {
                let row = &l.weights[o * l.inputs..(o + 1) * l.inputs];
                let sum = row
                    .iter()
                    .zip(a.iter())
                    .fold(l.bias[o], |s, (w, x)| s + w * x);
                b.push(if k == last {
                    match self.squash {
                        Squash::Tanh => sum.tanh(),
                        Squash::Clip => sum.clamp(-1.0, 1.0),
                    }
                } else {
                    self.activation.apply(sum)
                });
            }
            std::mem::swap(a, b);
        }
        out.copy_from_slice(a);
    }

    /// How many actions it gives: its task's (a car's two, a multirotor's four sticks).
    pub fn action_len(&self) -> usize {
        self.action_names().len()
    }

    /// The names of its actions: the sticks for a multirotor's task.
    fn action_names(&self) -> Vec<&'static str> {
        match Task::parse(&self.task) {
            Some(t) => self.action.fields_for(&t).iter().map(|f| f.name).collect(),
            None => self.action.fields().iter().map(|f| f.name).collect(),
        }
    }

    fn header(&self) -> Json {
        let strings = |v: Vec<&str>| Json::Arr(v.into_iter().map(Json::str).collect());
        let mut sizes = vec![Json::num(self.layers[0].inputs as f64)];
        sizes.extend(self.layers.iter().map(|l| Json::num(l.outputs as f64)));
        Json::obj(vec![
            ("format", Json::num(FORMAT as f64)),
            ("task", Json::str(self.task.as_str())),
            (
                "observation",
                strings(self.observation.iter().map(String::as_str).collect()),
            ),
            (
                "actions",
                strings(self.action_names()),
            ),
            ("action_mode", Json::str(self.action.mode.name())),
            ("decision_dt", Json::Num(self.decision_dt)),
            ("sizes", Json::Arr(sizes)),
            ("activation", Json::str(self.activation.name())),
            ("output", Json::str(self.squash.name())),
            ("normalize", Json::Bool(self.normalization.is_some())),
        ])
    }

    /// The file's bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let header = self.header().to_string();
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(header.len() as u32).to_le_bytes());
        out.extend_from_slice(header.as_bytes());
        let mut put = |v: &[f32]| {
            v.iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes()))
        };
        if let Some((mean, std)) = &self.normalization {
            put(mean);
            put(std);
        }
        for l in &self.layers {
            put(&l.weights);
            put(&l.bias);
        }
        out
    }

    /// Reads a model. Checks the file against itself, not against this build's observation
    /// layout: that is `check_layout`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Model, String> {
        if bytes.len() < 12 || &bytes[..8] != MAGIC {
            return Err(bad("not a flight model file"));
        }
        let header_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let header = bytes
            .get(12..12 + header_len)
            .and_then(|h| std::str::from_utf8(h).ok())
            .ok_or_else(|| bad("truncated header"))?;
        let h = Json::parse(header).map_err(bad)?;
        let text = |key: &str| {
            h.get(key)
                .and_then(Json::as_str)
                .ok_or_else(|| bad(format!("header has no {key}")))
        };
        let names = |key: &str| -> Result<Vec<String>, String> {
            h.get(key)
                .and_then(Json::as_array)
                .and_then(|a| a.iter().map(|x| x.as_str().map(str::to_string)).collect())
                .ok_or_else(|| bad(format!("header has no {key}")))
        };
        let format = h.get("format").and_then(Json::as_u64);
        if format != Some(FORMAT) {
            return Err(bad(format!("format {format:?} is not {FORMAT}")));
        }
        let mode = ActionMode::parse(text("action_mode")?).ok_or_else(|| bad("bad action_mode"))?;
        let actions = names("actions")?;
        let action = ActionConfig::from_names(
            mode,
            &actions.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .ok_or_else(|| bad(format!("unknown action layout {actions:?}")))?;
        let activation = match text("activation")? {
            "tanh" => Activation::Tanh,
            "relu" => Activation::Relu,
            other => return Err(bad(format!("unknown activation {other}"))),
        };
        let squash = match text("output")? {
            "tanh" => Squash::Tanh,
            "clip" => Squash::Clip,
            other => return Err(bad(format!("unknown output {other}"))),
        };
        let sizes: Vec<usize> = h
            .get("sizes")
            .and_then(Json::as_array)
            .and_then(|a| a.iter().map(|x| x.as_u64().map(|n| n as usize)).collect())
            .ok_or_else(|| bad("header has no sizes"))?;
        if sizes.len() < 2 || sizes.iter().any(|&n| n == 0 || n > 65_536) {
            return Err(bad("bad sizes"));
        }
        let observation = names("observation")?;
        let normalize = h.get("normalize").and_then(Json::as_bool).unwrap_or(false);

        let mut data = bytes[12 + header_len..].as_chunks::<4>().0.iter();
        let mut take = |n: usize| -> Result<Vec<f32>, String> {
            let v: Vec<f32> = data
                .by_ref()
                .take(n)
                .map(|c| f32::from_le_bytes(*c))
                .collect();
            if v.len() == n && v.iter().all(|x| x.is_finite()) {
                Ok(v)
            } else {
                Err(bad("truncated or not finite weights"))
            }
        };
        let normalization = if normalize {
            Some((take(sizes[0])?, take(sizes[0])?))
        } else {
            None
        };
        let mut layers = Vec::new();
        for s in sizes.windows(2) {
            layers.push(Layer {
                inputs: s[0],
                outputs: s[1],
                weights: take(s[0] * s[1])?,
                bias: take(s[1])?,
            });
        }
        let expected = 12
            + header_len
            + 4 * (layers
                .iter()
                .map(|l| l.weights.len() + l.bias.len())
                .sum::<usize>()
                + if normalize { 2 * sizes[0] } else { 0 });
        if bytes.len() != expected {
            return Err(bad(format!("{} bytes, expected {expected}", bytes.len())));
        }
        let model = Model {
            task: text("task")?.to_string(),
            observation,
            action,
            decision_dt: h
                .get("decision_dt")
                .and_then(Json::as_f64)
                .ok_or_else(|| bad("header has no decision_dt"))?,
            activation,
            squash,
            normalization,
            layers,
        };
        model.validate()?;
        Ok(model)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        std::fs::write(path, self.to_bytes())
    }

    /// Reads a model file and checks that it fits this build's observation layout.
    pub fn load(path: impl AsRef<Path>) -> Result<Model, String> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let model = Model::from_bytes(&bytes)?;
        model.check_layout()?;
        Ok(model)
    }
}

/// Working memory of `Model::act`.
#[derive(Clone, Debug, Default)]
pub struct Scratch {
    a: Vec<f32>,
    b: Vec<f32>,
}

/// A hand-written "model" for the `hold` task, to prove the whole path before anything is
/// trained: one linear layer that levels the wings, turns towards the target heading, holds the
/// target altitude with the elevator and the target airspeed with the throttle.
/// Direct actions, output clipped.
pub fn wings_leveller() -> Model {
    let action = ActionConfig {
        mode: ActionMode::Direct,
        flaps: false,
        trim: false,
    };
    let mut m = Model::new("hold", action, &[], Activation::Tanh);
    m.squash = Squash::Clip;
    let at = |name: &str| observation::index(name).expect("a field of the layout");
    let l = &mut m.layers[0];
    // Rows: 0 elevator, 1 aileron, 2 rudder, 3 throttle. Inputs are the scaled observation.
    // Elevator: pitch towards the altitude (through the vertical speed), damped.
    *l.weight_mut(0, at("altitude_error")) = 0.25;
    *l.weight_mut(0, at("vertical_speed")) = -0.9;
    *l.weight_mut(0, at("pitch_rate")) = -0.6;
    // Aileron: bank towards the heading, level otherwise.
    *l.weight_mut(1, at("heading_error")) = 0.5;
    *l.weight_mut(1, at("roll")) = -1.0;
    *l.weight_mut(1, at("roll_rate")) = -0.3;
    // Rudder: keep the ball centred and damp the yaw.
    *l.weight_mut(2, at("beta")) = 0.3;
    *l.weight_mut(2, at("yaw_rate")) = -0.3;
    // Throttle: cruise power, more when slow or low.
    l.bias[3] = 0.15;
    *l.weight_mut(3, at("speed_error")) = 1.5;
    *l.weight_mut(3, at("altitude_error")) = 0.3;
    m
}

/// Flies an aircraft or a multirotor (a model of `hover` or `gates`) with a `Model`. Decides every `model.decision_dt` and holds the controls in
/// between, as the environment does.
pub struct ModelPilot {
    model: Model,
    guidance: Option<Guidance>,
    mapper: ActionMapper,
    substeps: usize,
    /// Simulation steps since the last decision.
    count: usize,
    started: bool,
    targets: Targets,
    obs: Vec<f32>,
    action: Vec<f32>,
    scratch: Scratch,
    /// A `drive` model's circuit and car (`ModelPilot::driving`), and the rain its line is for.
    drive: Option<(Arc<Circuit>, CarType)>,
    rain: f64,
}

impl ModelPilot {
    /// A pilot for a model. What it is asked to do (which runway, which heading) is worked out
    /// from the model's task when it first sees the aircraft: see `Guidance::for_task`.
    ///
    /// Fails if the model was trained on another observation layout.
    pub fn new(model: Model) -> Result<ModelPilot, String> {
        model.validate()?;
        model.check_layout()?;
        Ok(ModelPilot {
            guidance: None,
            mapper: ActionMapper::new(model.action),
            substeps: ((model.decision_dt / World::DT).round() as usize).max(1),
            count: 0,
            started: false,
            targets: Targets::default(),
            obs: vec![0.0; model.fields().len()],
            action: vec![0.0; model.action_len()],
            scratch: Scratch::default(),
            drive: None,
            rain: 0.0,
            model,
        })
    }

    /// A pilot for a `drive` model, round `circuit` in a car of type `car`: its guidance (where it
    /// is on the circuit, the racing line ahead, with its speeds for the rain) is made when it first
    /// sees the car, and made again when the rain changes.
    pub fn driving(model: Model, circuit: Arc<Circuit>, car: &CarType) -> Result<ModelPilot, String> {
        if model.task != "drive" {
            return Err(format!("a {} model does not drive", model.task));
        }
        let mut p = ModelPilot::new(model)?;
        p.drive = Some((circuit, car.clone()));
        Ok(p)
    }

    /// Reads the model from a file.
    pub fn load(path: impl AsRef<Path>) -> Result<ModelPilot, String> {
        ModelPilot::new(Model::load(path)?)
    }

    /// Tells the pilot what to do instead of letting it work it out.
    pub fn with_guidance(mut self, guidance: Guidance) -> ModelPilot {
        self.guidance = Some(guidance);
        self
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// What it is being asked for, as of its last decision.
    pub fn targets(&self) -> &Targets {
        &self.targets
    }

    /// The observation vector of its last decision.
    pub fn observation(&self) -> &[f32] {
        &self.obs
    }

    /// Call when the aircraft is placed somewhere new. Forgets the guidance too (it is worked out
    /// again from the task), unless one is given with `with_guidance` afterwards.
    pub fn reset(&mut self) {
        self.guidance = None;
        self.started = false;
        self.count = 0;
    }
}

impl ModelPilot {
    /// `Pilot::fly` for an aircraft.
    pub fn fly_aircraft(&mut self, obs: &Observation, current: &AircraftControls) -> AircraftControls {
        if !self.started {
            self.started = true;
            self.count = 0;
            self.mapper.reset(current);
            if self.guidance.is_none() {
                self.guidance = Guidance::for_task(&self.model.task, obs);
            }
        }
        if !self.count.is_multiple_of(self.substeps) {
            self.count += 1;
            return *current;
        }
        self.count = 1;
        // Without guidance (no airport in sight for a runway task) the task terms are zero.
        self.targets = self
            .guidance
            .as_mut()
            .map(|g| g.update(obs))
            .unwrap_or_default();
        observation::write(
            &Inputs {
                obs,
                controls: &Controls::Aircraft(*current),
                targets: &self.targets,
            },
            &mut self.obs,
        );
        self.model
            .act(&self.obs, &mut self.scratch, &mut self.action);
        self.mapper
            .apply(&self.action, current, self.substeps as f64 * World::DT)
    }
}

impl ModelPilot {
    /// `Pilot::fly` for a multirotor (a model of `hover` or `gates`): the sticks.
    pub fn fly_multirotor(&mut self, obs: &Observation, current: &MultirotorControls) -> MultirotorControls {
        if !self.started {
            self.started = true;
            self.count = 0;
        }
        if !self.count.is_multiple_of(self.substeps) {
            self.count += 1;
            return *current;
        }
        self.count = 1;
        // `hover` and `gates` need a `Guidance` given (the point, the course): without one the
        // task terms are zero.
        self.targets = self.guidance.as_mut().map(|g| g.update(obs)).unwrap_or_default();
        observation::write_with(
            self.model.fields(),
            &Inputs { obs, controls: &Controls::Multirotor(*current), targets: &self.targets },
            &mut self.obs,
        );
        self.model.act(&self.obs, &mut self.scratch, &mut self.action);
        self.model.action.sticks(&self.action, current, self.substeps as f64 * World::DT)
    }
}

impl ModelPilot {
    /// `Pilot::fly` for a car (a model of `drive`): the steering and the pedal, the automatic gearbox.
    pub fn fly_car(&mut self, obs: &Observation, current: &CarControls) -> CarControls {
        if !self.started {
            self.started = true;
            self.count = 0;
        }
        if !self.count.is_multiple_of(self.substeps) {
            self.count += 1;
            return *current;
        }
        self.count = 1;
        // The guidance round its circuit, for the rain there is (the line's speeds follow it).
        let rain = obs.car().map_or(0.0, |c| c.rain);
        if let Some((circuit, car)) = &self.drive {
            if self.guidance.is_none() || (rain - self.rain).abs() > 0.025 {
                let along = circuit.locate(obs.north, obs.east).map_or(0.0, |a| a.along);
                let line = crate::task::wet_line(&crate::task::racing_line(circuit, car), car, rain);
                // Its progress so far is kept across a change of the line.
                let progress = self.targets.drive.map_or(0.0, |d| d.progress);
                self.guidance = Some(Guidance::drive(circuit.clone(), line, along).with_progress(progress));
                self.rain = rain;
            }
        }
        self.targets = self.guidance.as_mut().map(|g| g.update(obs)).unwrap_or_default();
        observation::write_with(self.model.fields(), &Inputs { obs, controls: &Controls::Car(*current), targets: &self.targets }, &mut self.obs);
        self.model.act(&self.obs, &mut self.scratch, &mut self.action);
        self.model.action.pedals(&self.action, current, self.substeps as f64 * World::DT)
    }
}

impl Pilot for ModelPilot {
    fn fly(&mut self, obs: &Observation, current: &Controls, _dt: f64) -> Controls {
        match current {
            Controls::Car(c) if self.model.task == "drive" => Controls::Car(self.fly_car(obs, c)),
            Controls::Aircraft(c) if !self.model.multirotor() => Controls::Aircraft(self.fly_aircraft(obs, c)),
            Controls::Multirotor(c) if self.model.multirotor() => Controls::Multirotor(self.fly_multirotor(obs, c)),
            // A model flies the kind of vehicle it was trained on: another it leaves alone.
            _ => *current,
        }
    }
}
