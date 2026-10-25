//! Queue rules for Wirecatch: pure functions of what they are given, so each rule is tested
//! without a running manager, which holds the queues and applies these rules to them.
//!
//! - [`plan`]: which downloads to start and stop so every queue's limit and the app's hold.
//! - [`schedule`]: when a scheduled queue may run.

#![forbid(unsafe_code)]

pub mod plan;
pub mod schedule;
