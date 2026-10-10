//! End-to-end tests of the download manager against a local HTTP server. Each
//! module covers part of the WP1 acceptance checklist.

mod common;
mod lifecycle;
mod limits;
mod profile;
mod protocol;
mod queues;
mod restart;
mod schedule;
