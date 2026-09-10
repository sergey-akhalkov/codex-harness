//! Native harness primitives. Read-only checks never build or acquire dependencies.
pub mod agent_config;
#[cfg(windows)]
pub mod broker_endpoint;
#[cfg(windows)]
pub mod broker_http;
#[cfg(windows)]
pub mod broker_launch;
#[cfg(windows)]
mod broker_requests;
#[cfg(windows)]
pub mod broker_rpc;
#[cfg(windows)]
pub mod broker_service;
#[cfg(windows)]
pub mod broker_state;
pub mod build_identity;
pub mod build_selection;
pub mod cancellable_pipe;
pub mod cbm_broker;
pub mod cbm_catalogue;
pub mod cbm_configuration;
pub mod cbm_index;
pub mod cbm_stdio;
pub mod codegraph_account;
pub mod codegraph_broker;
pub mod codegraph_catalogue;
pub mod codegraph_generation;
pub mod codegraph_integration;
pub mod codegraph_observer;
pub mod codegraph_registration;
pub mod codegraph_response;
pub mod codegraph_runtime;
pub mod codegraph_scheduler;
pub mod codegraph_stdio;
pub mod codegraph_store;
pub mod codegraph_transport;
pub mod config_create;
pub mod config_file;
pub mod console;
pub mod core_check;
pub mod core_disconnect;
pub mod core_install;
pub mod core_runtime;
pub mod dependency_archive;
mod dependency_assets;
pub mod dependency_audit;
pub mod dependency_candidate;
pub mod dependency_discovery;
mod dependency_fetch;
pub mod dependency_mcp_probe;
mod dependency_package;
pub mod dependency_plan;
mod dependency_probe;
pub mod dependency_process;
pub mod dependency_python_stage;
pub mod dependency_releases;
pub mod dependency_selection;
pub mod dependency_stage;
pub mod environment_path;
pub mod feature_edit;
#[cfg(windows)]
pub mod installation_links;
pub mod installation_lock;
mod installation_metadata;
mod installation_path;
pub mod installation_state;
pub mod inventory;
pub mod launcher;
mod legacy_pending;
mod legacy_pending_format;
pub mod mcp_protocol;
pub mod mcp_session;
pub mod mcp_stdio;
pub mod native_build;
pub mod native_launcher;
pub mod native_upstream;
pub mod opencodex;
pub mod outcome_report;
pub mod path_plan;
pub mod portable_config;
pub mod process;
mod process_path;
#[cfg(windows)]
pub mod process_service;
pub mod profile_state;
pub mod registration;
mod registration_native;
pub mod resource_admission;
pub mod source_observation;
mod wheel_record;
