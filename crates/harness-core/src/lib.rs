//! Native harness primitives. Read-only checks never build or acquire dependencies.
pub mod agent_config;
pub mod build_identity;
pub mod build_selection;
pub mod config_create;
pub mod config_file;
pub mod console;
pub mod environment_path;
pub mod feature_edit;
#[cfg(windows)]
pub mod installation_links;
pub mod installation_lock;
mod installation_metadata;
pub mod installation_state;
pub mod inventory;
pub mod launcher;
pub mod native_build;
pub mod native_launcher;
pub mod opencodex;
pub mod outcome_report;
pub mod path_plan;
pub mod process;
pub mod registration;
mod registration_native;
