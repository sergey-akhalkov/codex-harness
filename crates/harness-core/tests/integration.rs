//! One integration executable with a module per test area. Keep new test files
//! registered below: automatic discovery is disabled in the package manifest.
//! Test bodies, ignored cases and platform gates remain in their original files;
//! use the module prefix to select an area within this target.

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
mod infrastructure_accounting;
mod launcher;
mod link_changes;
mod process;
mod registration;
mod registration_finish;
mod serena;
mod skill_kit_inventory;
