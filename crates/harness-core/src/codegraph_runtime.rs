//! Native owner of an indexed project's worker and committed recovery checkpoint.
#![cfg(windows)]
use crate::{
    codegraph_catalogue,
    codegraph_generation::{ACTIVE_DIR_NAME, GenerationRole, GenerationStore, StorageLimits},
    codegraph_stdio::Configuration,
    codegraph_transport::{self, Admission, Monitor, Worker},
    process::{Cancellation, Deadline, Outcome},
};
use serde_json::{Value, json};
use std::{io, sync::Arc, time::Duration};

pub struct Runtime {
    configuration: Configuration,
    store: Option<GenerationStore>,
    admission: Option<Admission>,
    worker: Option<Worker>,
    generation: Option<u64>,
    failed: Option<String>,
    scheduled: bool,
    prepared: bool,
    worker_started: Option<std::time::Instant>,
    worker_used: Option<std::time::Instant>,
}

impl Runtime {
    pub fn new(configuration: Configuration) -> io::Result<Self> {
        let store = if configuration.data_name == ACTIVE_DIR_NAME {
            Some(GenerationStore::open(
                &configuration.project,
                StorageLimits::default(),
            )?)
        } else {
            None
        };
        Ok(Self {
            configuration,
            store,
            admission: None,
            worker: None,
            generation: None,
            failed: None,
            scheduled: false,
            prepared: false,
            worker_started: None,
            worker_used: None,
        })
    }

    pub(crate) fn scheduled(configuration: Configuration) -> io::Result<Self> {
        let mut runtime = Self::new(configuration)?;
        runtime.scheduled = true;
        Ok(runtime)
    }

    pub(crate) fn indexed(&self) -> io::Result<bool> {
        self.store
            .as_ref()
            .map_or(Ok(false), |store| Ok(store.committed_handle()?.is_some()))
    }

    pub(crate) fn failed(&self) -> bool {
        self.failed.is_some()
    }

    pub(crate) fn reconnect(&mut self) {
        // A new session after last-client retirement is a deliberate new lifetime.
        // Failed active data must be restored from the committed checkpoint.
        if self.failed.take().is_some() {
            self.prepared = false;
        }
    }

    pub(crate) fn retire_healthy(&mut self) -> io::Result<()> {
        if self
            .worker_started
            .is_some_and(|start| start.elapsed() >= Duration::from_secs(540))
            || self
                .worker_used
                .is_some_and(|used| used.elapsed() >= Duration::from_secs(60))
        {
            self.close()?;
        }
        Ok(())
    }

    pub fn status(&self) -> Value {
        json!({"provider":"codegraph","root":self.configuration.project,
            "worker":self.worker.is_some(),"failed":self.failed,"generation":self.generation})
    }

    fn monitor(&self) -> Option<Monitor> {
        self.store
            .clone()
            .map(|store| Arc::new(move || store.check_limits(None).map(|_| ())) as Monitor)
    }

    fn admit(&mut self) -> io::Result<()> {
        if self.admission.is_none() {
            self.admission = Some(Admission::acquire()?);
        }
        Ok(())
    }

    fn stop_worker(&mut self) -> io::Result<Option<Outcome>> {
        self.worker_started = None;
        self.worker_used = None;
        self.worker
            .take()
            .map(|mut worker| worker.close())
            .transpose()
            .map(Option::flatten)
    }

    pub fn close(&mut self) -> io::Result<Option<Outcome>> {
        let outcome = self.stop_worker()?;
        self.admission = None;
        Ok(outcome)
    }

    fn stamp(&self, mut value: Value, freshness: &str) -> Value {
        value["root"] = json!(self.configuration.project);
        value["generation"] = json!(self.generation);
        value["freshness"] = json!(freshness);
        value["coverage"] = json!("candidate-edges; upstream language/size/ignore limits apply");
        value
    }

    pub fn call(
        &mut self,
        name: &str,
        arguments: Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        let request = codegraph_catalogue::request(name, arguments)?;
        if name == "codegraph_detail" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "details belong to their stdio client",
            ));
        }
        if name == "codegraph_status" && self.failed.is_some() {
            return Ok(self.stamp(json!({"isError":true,"content":[{"type":"text",
                "text":"CodeGraph worker failed. Inspect the cause, then use codegraph_sync or codegraph_index deliberately."}],
                "error":self.failed}), "failed"));
        }
        if self.failed.is_some() && !matches!(name, "codegraph_index" | "codegraph_sync") {
            return Ok(self.stamp(json!({"isError":true,"content":[{"type":"text",
                "text":"CodeGraph worker failed; automatic restart is disabled. Use codegraph_sync deliberately after inspecting the failure."}],
                "error":self.failed}), "failed"));
        }
        let result = self.call_inner(
            name,
            request.arguments,
            Deadline::after(deadline.remaining().min(request.timeout))?,
            cancel,
        );
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                let diagnostics = self.worker.as_ref().map(Worker::failure_diagnostics);
                let cleanup = self.stop_worker()?;
                let mut recovery = None;
                if self.admission.is_some()
                    && let Some(store) = &self.store
                {
                    recovery = Some(
                        match store
                            .rollback_failed_stage()
                            .and_then(|_| store.committed_handle())
                        {
                            Ok(handle) => {
                                let generation = handle.and_then(|h| h.generation);
                                json!({"saved_generation":generation,"freshness":if generation.is_some() {"stale"} else {"unindexed"},"next_action":if generation.is_some() {"codegraph_sync restores the saved checkpoint and catches up"} else {"codegraph_index creates the first usable checkpoint"}})
                            }
                            Err(cause) => {
                                json!({"error":cause.to_string(),"committed_checkpoint_preserved":true})
                            }
                        },
                    );
                }
                self.admission = None;
                self.prepared = false;
                let busy = error.kind() == io::ErrorKind::WouldBlock;
                if !busy {
                    self.failed = Some(error.to_string());
                }
                Ok(self.stamp(json!({"isError":true,"error":{"category":format!("{:?}",error.kind()),
                    "message":error.to_string()},"content":[{"type":"text","text":error.to_string()}],
                    "diagnostics":diagnostics,"cleanup":cleanup,"recovery":recovery}),
                    if busy { "stale" } else { "failed" }))
            }
        }
    }

    fn call_inner(
        &mut self,
        name: &str,
        arguments: Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        if matches!(name, "codegraph_index" | "codegraph_sync") {
            return self.update(name, deadline, cancel);
        }
        if let Some(store) = &self.store
            && store.committed_handle()?.is_none()
        {
            return Ok(self.stamp(json!({"isError":name != "codegraph_status",
                    "content":[{"type":"text","text":"This exact project root has no committed CodeGraph index. Run codegraph_index deliberately to create it."}]}), "unindexed"));
        }
        if self.worker.is_none() {
            // Admission covers restoration too: another client can never
            // replace an active database while the account worker owns it.
            self.admit()?;
            if let Some(store) = &self.store
                && (!self.scheduled || !self.prepared)
            {
                let handle = store.startup_active_bounded(deadline, cancel)?;
                self.generation = handle.generation;
                self.prepared = true;
            }
            let mut command = self.configuration.command()?;
            if self.scheduled {
                command.args.push("--no-watch".into());
                command
                    .env
                    .insert("CODEGRAPH_FORCE_WATCH".into(), Some("0".into()));
            }
            let worker = Worker::start_admitted(
                command,
                Duration::from_secs(600),
                cancel,
                self.monitor(),
                self.admission.as_ref().unwrap().clone(),
            )?;
            self.worker = Some(worker);
            self.worker_started = Some(std::time::Instant::now());
            self.worker.as_mut().unwrap().initialize(deadline, cancel)?;
        }
        let root = json!(self.configuration.project);
        let worker = self.worker.as_mut().unwrap();
        let mut arguments = arguments;
        arguments["projectPath"] = root.clone();
        let result = worker.tool(name, arguments, deadline, cancel)?;
        self.worker_used = Some(std::time::Instant::now());
        if self.store.is_none() {
            return Ok(result);
        }
        if self.scheduled {
            // Native observation and completed finite sync episodes own freshness.
            // The scheduler adds pending state if a newer event arrived.
            return Ok(self.stamp(result, "complete"));
        }
        // Status uses the watched main-thread instance. A watcher being present
        // alone is insufficient: pending files and interrupted resolution must
        // stay visible even when the requested query itself succeeded.
        let status = if name == "codegraph_status" {
            result.clone()
        } else {
            worker.tool(
                "codegraph_status",
                json!({"projectPath":root}),
                deadline,
                cancel,
            )?
        };
        let text = status["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let diagnostics = worker.diagnostics();
        let freshness = if status["isError"] == true {
            "unverified"
        } else if text.contains("**Auto-sync disabled:") {
            "failed"
        } else if text.contains("**Pending sync:") || text.contains("**Pending resolution:") {
            "pending"
        } else if diagnostics["watcher_active"] == true {
            "complete"
        } else {
            "unverified"
        };
        let mut result = self.stamp(result, freshness);
        result["observed_refreshes"] = diagnostics["completed_refreshes"].clone();
        result["checkpoint"] = json!("last-explicit-index-or-sync");
        if matches!(freshness, "pending" | "failed") {
            result["refresh_status"] = status;
        }
        if freshness == "failed" {
            return Err(io::Error::other(
                "CodeGraph automatic refresh is disabled; use explicit sync after inspecting the cause",
            ));
        }
        Ok(result)
    }

    fn update(
        &mut self,
        name: &str,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        let store = self.store.clone().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "managed index/sync requires the owned generation store",
            )
        })?;
        self.admit()?;
        self.stop_worker()?;
        let full = name == "codegraph_index";
        if !full && store.committed_handle()?.is_none() {
            self.admission = None;
            return Ok(self.stamp(
                json!({"isError":true,"content":[{"type":"text",
                "text":"No committed index exists. Run codegraph_index first."}]}),
                "unindexed",
            ));
        }
        let handle = if full {
            store.stage_full_rebuild()?
        } else if self.scheduled && self.prepared {
            store
                .active_handle()?
                .ok_or_else(|| io::Error::other("CodeGraph active generation disappeared"))?
        } else {
            store.startup_active_bounded(deadline, cancel)?
        };
        let mut configuration = self.configuration.clone();
        configuration.data_name = handle.data_name;
        let result = codegraph_transport::run_command_admitted(
            configuration.cli_command(if full { "index" } else { "sync" })?,
            deadline,
            cancel,
            self.monitor(),
            self.admission.as_ref().unwrap().clone(),
        )?;
        if result.outcome.exit_code != 0
            || result.monitor_failure.is_some()
            || result.omitted_stdout_bytes != 0
            || result.omitted_stderr_bytes != 0
        {
            return Err(io::Error::other(format!(
                "CodeGraph {name} did not complete: {}",
                serde_json::to_string(&result)?
            )));
        }
        let counts =
            crate::codegraph_store::validate_completion(&handle.database, deadline, cancel)?;
        store.check_limits(None)?;
        let committed = store.commit_bounded(
            if full {
                GenerationRole::Stage
            } else {
                GenerationRole::Active
            },
            deadline,
            cancel,
        )?;
        self.generation = committed.generation;
        self.prepared = true;
        self.failed = None;
        self.admission = None;
        Ok(self.stamp(json!({"content":[{"type":"text","text":"CodeGraph index checkpoint committed. Connected projects receive bounded automatic catch-up."}],
            "counts":counts,"operation":name,"diagnostics":result}), "complete"))
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Explicit package qualification for the retained dependency lifecycle. This
/// starts only the verified published runtime in a new owned unindexed root;
/// normal discovery/startup never invokes this probe or downloads anything.
pub fn probe_package(package: &std::path::Path) -> io::Result<Value> {
    let inspected = crate::dependency_discovery::inspect_package(package)?;
    let directory = tempfile::Builder::new()
        .prefix("harness-codegraph-probe-")
        .tempdir()?;
    let configuration = crate::codegraph_stdio::configuration(
        &inspected.node,
        &inspected.entry,
        directory.path(),
        format!(
            ".codegraph-probe-{}",
            &crate::broker_endpoint::random_key()?[..16]
        ),
    )?;
    let deadline = Deadline::after(Duration::from_secs(30))?;
    let cancel = Cancellation::default();
    let mut worker = Worker::start(
        configuration.command_policy(false)?,
        Duration::from_secs(30),
        &cancel,
    )?;
    let result = (|| {
        let initialized = worker.initialize(deadline, &cancel)?;
        let catalogue = worker.request("tools/list", json!({}), deadline, &cancel)?;
        let definitions = catalogue["result"]["tools"]
            .as_array()
            .ok_or_else(|| io::Error::other("CodeGraph package probe omitted its catalogue"))?;
        if definitions.is_empty() {
            return Err(io::Error::other(
                "CodeGraph package probe returned no tools",
            ));
        }
        Ok(
            json!({"kind":"published-direct-mcp","server_info":initialized["serverInfo"],
            "tool_count":definitions.len(),"project":"owned-unindexed-probe","downloaded":false,"indexed":false}),
        )
    })();
    let cleanup = worker.close()?;
    let mut report = result?;
    report["cleanup"] = json!(cleanup);
    directory.close()?;
    Ok(report)
}
