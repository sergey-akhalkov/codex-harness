//! Native harness primitives. Read-only checks never build or acquire dependencies.
pub mod agent_config;
pub mod analysis_samples;
pub mod benefit_gate;
pub mod board_cli;
pub mod board_feedback;
#[cfg(windows)]
pub mod board_lifecycle;
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
#[cfg(windows)]
pub mod code_tools_lifecycle;
pub mod config_create;
pub mod config_file;
pub mod console;
pub mod core_check;
pub mod core_disconnect;
pub mod core_install;
pub mod core_runtime;
pub mod dependency_apply;
pub mod dependency_archive;
mod dependency_assets;
pub mod dependency_audit;
pub mod dependency_candidate;
pub mod dependency_discovery;
mod dependency_fetch;
pub mod dependency_mcp_probe;
pub mod dependency_npm_install;
mod dependency_package;
pub mod dependency_plan;
mod dependency_probe;
pub mod dependency_process;
pub mod dependency_python_stage;
pub mod dependency_releases;
pub mod dependency_rust_component;
pub mod dependency_selection;
pub mod dependency_stage;
pub mod environment_path;
pub mod executable_ownership;
pub mod feature_edit;
pub mod heavy_command;
#[cfg(windows)]
pub mod installation_links;
pub mod installation_lock;
mod installation_metadata;
mod installation_path;
pub mod installation_reset;
pub mod installation_state;
pub mod inventory;
pub mod launcher;
pub mod lazy_stdio;
mod legacy_pending;
mod legacy_pending_format;
#[cfg(windows)]
pub mod lifecycle;
pub mod mcp_preparation;
pub mod mcp_protocol;
pub mod mcp_registration;
pub mod mcp_session;
pub mod mcp_stdio;
pub mod native_build;
pub mod native_launcher;
pub mod native_upstream;
pub mod nuphus_protocol;
#[cfg(windows)]
pub mod nuphus_stdio;
#[cfg(windows)]
pub mod opencodex_login;
pub mod orchestration_config;
pub mod orchestration_lifecycle;
pub mod outcome_report;
pub mod pacing;
pub mod path_plan;
pub mod portable_config;
pub mod process;
mod process_path;
#[cfg(windows)]
pub mod process_service;
pub mod profile_state;
pub mod registration;
mod registration_native;
pub mod report_owners;
pub mod resource_admission;
pub mod rollout_reader;
pub mod scoped_observations;
#[cfg(windows)]
pub mod serena;
pub mod serena_broker;
pub mod serena_configuration;
pub mod serena_route;
pub mod serena_shared;
pub mod serena_stdio;
pub mod source_observation;
#[cfg(windows)]
pub mod subscription_lifecycle;
#[cfg(windows)]
pub mod subscription_login;
#[cfg(windows)]
#[cfg(windows)]
pub mod task_admission;
#[cfg(windows)]
pub mod task_arguments;
#[cfg(windows)]
mod task_child_views;
pub mod task_control;
pub mod task_failure;
#[cfg(windows)]
pub mod task_forward;
#[cfg(windows)]
pub mod task_gateway;
#[cfg(windows)]
mod task_handoff;
#[cfg(windows)]
mod task_observer;
pub mod task_orchestrate;
pub mod task_request;
#[cfg(windows)]
pub mod task_runtime;
#[cfg(windows)]
pub mod task_scheduler;
pub mod task_store;
pub mod task_succession;
#[cfg(windows)]
pub mod task_view;
pub mod task_worktree;
#[cfg(windows)]
pub mod token_workflow_lifecycle;
mod wheel_record;
#[cfg(windows)]
pub mod xai_responses_probe;
#[cfg(windows)]
pub mod xai_responses_shim;
#[cfg(windows)]
pub mod xai_token_helper;
