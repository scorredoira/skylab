//! The action vector and how it becomes `AircraftControls`, or a multirotor's sticks
//! (`MultirotorControls`) for the multirotor's task. Shared by the environment and `ModelPilot`.

use sim_core::{AircraftControls, CarControls, MultirotorControls};

use crate::task::Task;

/// What an action means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionMode {
    /// The action is where each control goes.
    Direct,
    /// The action is how fast each control moves; it is integrated and clamped.
    Incremental,
}

impl ActionMode {
    pub fn name(self) -> &'static str {
        match self {
            ActionMode::Direct => "direct",
            ActionMode::Incremental => "incremental",
        }
    }

    pub fn parse(s: &str) -> Option<ActionMode> {
        match s {
            "direct" => Some(ActionMode::Direct),
            "incremental" => Some(ActionMode::Incremental),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionConfig {
    pub mode: ActionMode,
    /// Adds a fifth action that moves the flap lever. Without it the flaps stay as they are.
    pub flaps: bool,
    /// Adds an action that moves the elevator trim. Without it the trim stays as it is.
    pub trim: bool,
}

impl Default for ActionConfig {
    fn default() -> ActionConfig {
        ActionConfig {
            mode: ActionMode::Incremental,
            flaps: false,
            trim: false,
        }
    }
}

/// One entry of the action vector. Every action is in [-1, 1] (values outside are clamped).
pub struct ActionField {
    pub name: &'static str,
    pub direct: &'static str,
    pub incremental: &'static str,
}

/// In incremental mode, a full action moves the yoke and pedals this much per second.
pub const SURFACE_RATE: f64 = 1.0;
/// ... the throttle this much per second (it goes from 0 to 1).
pub const THROTTLE_RATE: f64 = 0.5;
/// ... the flap lever this many notches per second.
pub const FLAP_RATE: f64 = 1.0;
/// ... the trim wheel this much per second.
pub const TRIM_RATE: f64 = 0.1;
/// In direct mode the throttle is `0.5 + THROTTLE_GAIN * action`, clamped: a little beyond the
/// ends, so closed (which is also what brakes on the ground) and full open can be reached exactly.
pub const THROTTLE_GAIN: f64 = 0.55;

const ELEVATOR: ActionField = ActionField {
    name: "elevator",
    direct: "yoke position: +1 full back (nose up)",
    incremental: "yoke speed: 1 moves it back at 1 per second",
};
const AILERON: ActionField = ActionField {
    name: "aileron",
    direct: "yoke position: +1 full right",
    incremental: "yoke speed: 1 moves it right at 1 per second",
};
const RUDDER: ActionField = ActionField {
    name: "rudder",
    direct: "pedal position: +1 full right",
    incremental: "pedal speed: 1 moves it right at 1 per second",
};
const THROTTLE: ActionField = ActionField {
    name: "throttle",
    direct: "throttle = clamp(0.5 + 0.55 a, 0, 1); closed below -0.91 (the wheels brake when it is closed on the ground)",
    incremental: "throttle speed: 1 opens it at 0.5 per second",
};
const FLAPS: ActionField = ActionField {
    name: "flaps",
    direct: "flap notch = round(1.5 (a + 1)): 0, 10, 20, 30 degrees",
    incremental:
        "flap lever speed: 1 moves it up one notch per second; the notch is the lever rounded",
};
const TRIM: ActionField = ActionField {
    name: "trim",
    direct: "elevator trim: +1 full nose up",
    incremental: "trim wheel speed: 1 moves it nose up at 0.1 per second",
};

/// In incremental mode, a full action moves a multirotor's stick this much per second.
pub const STICK_RATE: f64 = 2.0;

const STICK_THROTTLE: ActionField = ActionField {
    name: "throttle",
    direct: "throttle stick = (a + 1) / 2: 0 down, 1 up",
    incremental: "throttle stick speed: 1 raises it at 1 per second",
};
const STICK_ROLL: ActionField = ActionField {
    name: "roll",
    direct: "roll stick: +1 full right",
    incremental: "roll stick speed: 1 moves it right at 2 per second",
};
const STICK_PITCH: ActionField = ActionField {
    name: "pitch",
    direct: "pitch stick: +1 full back (nose up)",
    incremental: "pitch stick speed: 1 moves it back at 2 per second",
};
const STICK_YAW: ActionField = ActionField {
    name: "yaw",
    direct: "yaw stick: +1 full right",
    incremental: "yaw stick speed: 1 moves it right at 2 per second",
};

const STEER: ActionField = ActionField {
    name: "steer",
    direct: "steering: -1 full left, +1 full right",
    incremental: "steering speed: 1 turns it right at 2 per second",
};
const PEDAL: ActionField = ActionField {
    name: "pedal",
    direct: "pedals: above 0 the throttle, below 0 the brake",
    incremental: "pedal speed: 1 moves it towards full throttle at 4 per second",
};

impl ActionConfig {
    /// The actions of a task: a multirotor's four sticks for `hover`, the aircraft's controls
    /// (`fields`) for the rest.
    pub fn fields_for(&self, task: &Task) -> Vec<&'static ActionField> {
        if task.car() {
            vec![&STEER, &PEDAL]
        } else if task.multirotor() {
            vec![&STICK_THROTTLE, &STICK_ROLL, &STICK_PITCH, &STICK_YAW]
        } else {
            self.fields()
        }
    }

    /// Length of a task's action vector.
    pub fn len_for(&self, task: &Task) -> usize {
        self.fields_for(task).len()
    }

    /// A multirotor's sticks after applying `action` (throttle, roll, pitch, yaw) for `dt`
    /// seconds; the arm switch as it is.
    pub fn sticks(&self, action: &[f32], current: &MultirotorControls, dt: f64) -> MultirotorControls {
        assert_eq!(action.len(), 4);
        let a = |i: usize| -> f64 {
            let x = f64::from(action[i]);
            if x.is_finite() {
                x.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        };
        let mut c = *current;
        match self.mode {
            ActionMode::Direct => {
                c.throttle = (a(0) + 1.0) / 2.0;
                (c.roll, c.pitch, c.yaw) = (a(1), a(2), a(3));
            }
            ActionMode::Incremental => {
                c.throttle = (c.throttle + a(0) * dt).clamp(0.0, 1.0);
                c.roll = (c.roll + a(1) * STICK_RATE * dt).clamp(-1.0, 1.0);
                c.pitch = (c.pitch + a(2) * STICK_RATE * dt).clamp(-1.0, 1.0);
                c.yaw = (c.yaw + a(3) * STICK_RATE * dt).clamp(-1.0, 1.0);
            }
        }
        c
    }

    /// A car's pedals and steering after applying `action` (steer, pedal) for `dt` seconds: the
    /// pedal is the throttle above 0 and the brake below. `direct`: where they are; `incremental`:
    /// the steering moves at 2 a second, the pedal at 4. The gearbox is the automatic.
    pub fn pedals(&self, action: &[f32], current: &CarControls, dt: f64) -> CarControls {
        assert_eq!(action.len(), 2);
        let a = |i: usize| -> f64 {
            let x = f64::from(action[i]);
            if x.is_finite() {
                x.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        };
        let mut c = *current;
        let pedal = match self.mode {
            ActionMode::Direct => {
                c.steer = a(0);
                a(1)
            }
            ActionMode::Incremental => {
                c.steer = (c.steer + a(0) * 2.0 * dt).clamp(-1.0, 1.0);
                (c.throttle - c.brake + a(1) * 4.0 * dt).clamp(-1.0, 1.0)
            }
        };
        c.throttle = pedal.max(0.0);
        c.brake = (-pedal).max(0.0);
        c.automatic = true;
        c
    }

    /// The aircraft's actions.
    pub fn fields(&self) -> Vec<&'static ActionField> {
        let mut out: Vec<&'static ActionField> = vec![&ELEVATOR, &AILERON, &RUDDER, &THROTTLE];
        if self.flaps {
            out.push(&FLAPS);
        }
        if self.trim {
            out.push(&TRIM);
        }
        out
    }

    /// Length of the action vector.
    pub fn len(&self) -> usize {
        4 + usize::from(self.flaps) + usize::from(self.trim)
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// The configuration whose action names are `names`, if there is one.
    /// A multirotor's sticks (`throttle`, `roll`, `pitch`, `yaw`) are the configuration without
    /// flaps or trim.
    pub fn from_names(mode: ActionMode, names: &[&str]) -> Option<ActionConfig> {
        let sticks = [&STICK_THROTTLE, &STICK_ROLL, &STICK_PITCH, &STICK_YAW];
        if sticks.iter().map(|f| f.name).eq(names.iter().copied()) {
            return Some(ActionConfig { mode, flaps: false, trim: false });
        }
        // A car's (`drive`): the steering and the pedal.
        if [&STEER, &PEDAL].iter().map(|f| f.name).eq(names.iter().copied()) {
            return Some(ActionConfig { mode, flaps: false, trim: false });
        }
        [(false, false), (true, false), (false, true), (true, true)]
            .into_iter()
            .map(|(flaps, trim)| ActionConfig { mode, flaps, trim })
            .find(|c| c.fields().iter().map(|f| f.name).eq(names.iter().copied()))
    }
}

/// Turns actions into controls. Holds the position of the flap lever between notches.
#[derive(Clone, Debug)]
pub struct ActionMapper {
    pub config: ActionConfig,
    flap_lever: f64,
}

impl ActionMapper {
    pub fn new(config: ActionConfig) -> ActionMapper {
        ActionMapper {
            config,
            flap_lever: 0.0,
        }
    }

    /// Call when the aircraft is placed somewhere new.
    pub fn reset(&mut self, current: &AircraftControls) {
        self.flap_lever = f64::from(current.flap_notch);
    }

    /// The controls after applying `action` (of length `config.len()`) for `dt` seconds.
    /// Anything the action does not cover is returned as it is in `current`.
    pub fn apply(&mut self, action: &[f32], current: &AircraftControls, dt: f64) -> AircraftControls {
        assert_eq!(action.len(), self.config.len());
        let a = |i: usize| -> f64 {
            let x = f64::from(action[i]);
            if x.is_finite() {
                x.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        };
        let mut c = *current;
        let flaps_at = 4;
        let trim_at = 4 + usize::from(self.config.flaps);
        match self.config.mode {
            ActionMode::Direct => {
                c.elevator = a(0);
                c.aileron = a(1);
                c.rudder = a(2);
                c.throttle = (0.5 + THROTTLE_GAIN * a(3)).clamp(0.0, 1.0);
                if self.config.flaps {
                    self.flap_lever = (1.5 * (a(flaps_at) + 1.0)).clamp(0.0, 3.0);
                }
                if self.config.trim {
                    c.trim = a(trim_at);
                }
            }
            ActionMode::Incremental => {
                let step = SURFACE_RATE * dt;
                c.elevator = (c.elevator + a(0) * step).clamp(-1.0, 1.0);
                c.aileron = (c.aileron + a(1) * step).clamp(-1.0, 1.0);
                c.rudder = (c.rudder + a(2) * step).clamp(-1.0, 1.0);
                c.throttle = (c.throttle + a(3) * THROTTLE_RATE * dt).clamp(0.0, 1.0);
                if self.config.flaps {
                    self.flap_lever =
                        (self.flap_lever + a(flaps_at) * FLAP_RATE * dt).clamp(0.0, 3.0);
                }
                if self.config.trim {
                    c.trim = (c.trim + a(trim_at) * TRIM_RATE * dt).clamp(-1.0, 1.0);
                }
            }
        }
        if self.config.flaps {
            c.flap_notch = self.flap_lever.round() as i32;
        }
        c
    }
}
