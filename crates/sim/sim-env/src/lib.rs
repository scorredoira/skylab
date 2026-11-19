//! A training environment over `sim-core`: tasks with rewards, a vectorised gym-style API, a
//! socket server for a trainer in another program, and `ModelPilot`, which flies a trained
//! network inside the simulator. See `README.md` and `PROTOCOL.md`.

pub mod action;
pub mod env;
pub mod guidance;
pub mod json;
pub mod model;
pub mod observation;
pub mod referee;
pub mod server;
pub mod task;
pub mod vec_env;

pub use action::{ActionConfig, ActionMapper, ActionMode};
pub use env::{Env, EnvConfig, Info, Step};
pub use guidance::{Guidance, Runway, RunwayView, Targets};
pub use model::{Activation, Layer, Model, ModelPilot, Squash};
pub use referee::Referee;
pub use task::{Reason, Task};
pub use vec_env::{performance_cores, Finished, VecEnv, VecStep};
