//! The daemon: startup, the poll loop, and the exit-code contract. The behavior lives in
//! [`crate::core`]; these modules only move bytes and time.

mod exit;
mod runtime;
mod startup;

pub(crate) use runtime::run;
pub(crate) use startup::runtime_dir;
