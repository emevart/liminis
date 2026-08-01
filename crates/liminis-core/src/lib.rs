//! Liminis core.
//!
//! The CPU reference implementation of the simulator. Nothing is simulated yet:
//! at this stage the crate holds the world semantics version and the scenario
//! config loader that turns a TOML file into a run identity.

pub mod config;
pub mod kernels;
pub mod numeric;
pub mod process;
pub mod version;
pub mod world;
