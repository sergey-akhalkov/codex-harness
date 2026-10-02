//! Pilot consolidation: one integration executable for the former harness-core
//! integration targets. Each original file is included as a module without
//! being moved, so test bodies, ignored cases and platform gates are
//! unchanged; test names gain the module prefix.
//!
//! Adopted by the measured comparison for reduce-rust-build-cost task 2.2:
//! fresh targets under the unchanged heavy budget, identical compact-dev
//! settings, cold compile payload 171.7s -> 148.7s, integration artifacts
//! 205.8 MB -> 37.4 MB, inventory 148 == 148 and execution 132 passed /
//! 16 ignored / 0 failed on both sides.

mod cancellable_pipe;
mod config_creation;
mod config_file;
mod feature_discovery;
mod feature_edit;
mod improvement_activation;
mod improvement_experiment;
mod improvement_intake;
mod improvement_policy;
mod improvement_runtime;
mod improvement_spec;
mod launcher;
mod link_changes;
mod process;
mod registration;
mod registration_finish;
mod serena;
mod skill_kit_inventory;
