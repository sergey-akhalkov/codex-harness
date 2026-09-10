//! One account-wide FIFO executor, with independently observed client projects.
#![cfg(windows)]
use crate::{
    codegraph_observer::Observer,
    codegraph_runtime::Runtime,
    codegraph_stdio::Configuration,
    dependency_discovery::local_path,
    process::{Cancellation, Deadline, ProcessIdentity},
    process_service::{ServiceProcess, current_user},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CAPACITY: usize = 64;
const TICK: Duration = Duration::from_millis(100);

pub(crate) struct Scheduler {
    requests: SyncSender<Request>,
    stop: Cancellation,
    live: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    status: Arc<Mutex<Value>>,
    thread: Mutex<Option<JoinHandle<io::Result<()>>>>,
}
struct Request {
    name: String,
    payload: Value,
    deadline: Deadline,
    cancel: Cancellation,
    reply: SyncSender<io::Result<Value>>,
    stage: Arc<AtomicU8>, // queued, running, abandoned
}
struct Client {
    root: PathBuf,
    process: ServiceProcess,
}
struct Project {
    runtime: Arc<Mutex<Runtime>>,
    observer: Option<Observer>,
    observed: u64,
    committed: u64,
    pending_since: Instant,
    last_event: Instant,
    queued: bool,
    last_client: Option<Instant>,
    observation_error: Option<String>,
    reconnect: bool,
}
enum Work {
    Request(Request),
    Refresh(PathBuf),
}
struct State {
    configuration: Configuration,
    projects: BTreeMap<PathBuf, Project>,
    clients: BTreeMap<String, Client>,
    queue: VecDeque<Work>,
    active: Option<PathBuf>,
    running: Option<Running>,
}

struct Running {
    root: PathBuf,
    before: u64,
    name: String,
    reply: Option<SyncSender<io::Result<Value>>>,
    cancel: Cancellation,
    caller: Cancellation,
    handle: JoinHandle<io::Result<Value>>,
}

impl Scheduler {
    pub fn start(configuration: Configuration) -> io::Result<Self> {
        let (requests, input) = mpsc::sync_channel(CAPACITY);
        let stop = Cancellation::default();
        let live = Arc::new(AtomicBool::new(false));
        let busy = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(json!({"provider":"codegraph","projects":[]})));
        let (stopping, alive, working, snapshot) =
            (stop.clone(), live.clone(), busy.clone(), status.clone());
        let thread = thread::Builder::new()
            .name("codegraph-scheduler".into())
            .spawn(move || {
                let mut state = State {
                    configuration,
                    projects: BTreeMap::new(),
                    clients: BTreeMap::new(),
                    queue: VecDeque::new(),
                    active: None,
                    running: None,
                };
                let result = state.run(input, &stopping, &alive, &working, &snapshot);
                if let Some(running) = &state.running {
                    running.cancel.cancel();
                }
                state.complete(true)?;
                for project in state.projects.values_mut() {
                    project
                        .runtime
                        .lock()
                        .map_err(|_| io::Error::other("CodeGraph runtime poisoned"))?
                        .close()?;
                    if let Some(observer) = &mut project.observer {
                        observer.close()?;
                    }
                }
                alive.store(false, Ordering::Release);
                result
            })?;
        Ok(Self {
            requests,
            stop,
            live,
            busy,
            status,
            thread: Mutex::new(Some(thread)),
        })
    }
    pub fn call(
        &self,
        name: &str,
        payload: &Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        let (reply, response) = mpsc::sync_channel(1);
        let stage = Arc::new(AtomicU8::new(0));
        match self.requests.try_send(Request {
            name: name.into(),
            payload: payload.clone(),
            deadline,
            cancel: cancel.clone(),
            reply,
            stage: stage.clone(),
        }) {
            Ok(()) => (),
            Err(mpsc::TrySendError::Full(_)) => {
                return Ok(
                    json!({"isError":true,"freshness":"pending","error":"CodeGraph account queue is full; retry within a new bounded request"}),
                );
            }
            Err(_) => return Err(io::Error::other("CodeGraph scheduler stopped")),
        }
        // The broker owns cancellation and cleanup deadlines. Do not ACK until
        // the executor has observed cancellation and reclaimed request resources.
        loop {
            match response.recv_timeout(TICK) {
                Ok(value) => return value,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::other(
                        "CodeGraph scheduler stopped before cleanup confirmation",
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => (),
            }
            if (cancel.is_cancelled() || deadline.expired())
                && stage
                    .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                return Ok(
                    json!({"isError":true,"freshness":"pending","error":"CodeGraph queued request cancelled or expired before admission"}),
                );
            }
        }
    }
    pub fn keep_alive(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
    pub fn is_idle(&self) -> bool {
        !self.busy.load(Ordering::Acquire)
    }
    pub fn status(&self) -> Value {
        self.status
            .try_lock()
            .map(|v| v.clone())
            .unwrap_or_else(|_| json!({"provider":"codegraph","busy":true}))
    }
    pub fn shutdown(&self) -> io::Result<()> {
        self.stop.cancel();
        if let Some(thread) = self
            .thread
            .lock()
            .map_err(|_| io::Error::other("CodeGraph scheduler owner poisoned"))?
            .take()
        {
            thread
                .join()
                .map_err(|_| io::Error::other("CodeGraph scheduler panicked"))??;
        }
        Ok(())
    }
}
impl Drop for Scheduler {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

impl State {
    fn run(
        &mut self,
        input: Receiver<Request>,
        stop: &Cancellation,
        live: &AtomicBool,
        busy: &AtomicBool,
        status: &Mutex<Value>,
    ) -> io::Result<()> {
        while !stop.is_cancelled() {
            self.complete(false)?;
            // Control registration is bounded and cheap; queue only graph work.
            // Both channel and FIFO are bounded even while an episode is active.
            for _ in 0..(CAPACITY / 2).saturating_sub(self.queue.len()) {
                let request = match input.try_recv() {
                    Ok(v) => v,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(_) => return Ok(()),
                };
                if matches!(request.name.as_str(), "client/open" | "client/close") {
                    if request
                        .stage
                        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                        .is_err()
                    {
                        continue;
                    }
                    let result = self
                        .control(&request)
                        .or_else(|error| Ok(json!({"isError":true,"error":error.to_string()})));
                    let _ = request.reply.send(result);
                } else {
                    self.queue.push_back(Work::Request(request));
                }
            }
            self.observe()?;
            if let Some(running) = &self.running
                && (running.caller.is_cancelled()
                    || !self
                        .clients
                        .values()
                        .any(|client| client.root == running.root))
            {
                running.cancel.cancel();
            }
            live.store(!self.clients.is_empty(), Ordering::Release);
            if let Ok(mut value) = status.lock() {
                *value = json!({"provider":"codegraph","clients":self.clients.len(),"queued":self.queue.len(),
                    "active_root":self.active,"projects":self.projects.iter().map(|(root, p)| json!({
                        "root":root,"observing":p.observer.is_some(),"pending":p.observed != p.committed,
                        "observation_error":p.observation_error,"runtime":p.runtime.try_lock().map(|runtime| runtime.status()).unwrap_or_else(|_| json!({"busy":true}))})).collect::<Vec<_>>()});
            }
            if self.running.is_none()
                && let Some(work) = self.queue.pop_front()
            {
                self.execute(work)?;
            }
            busy.store(self.running.is_some(), Ordering::Release);
            thread::sleep(TICK);
        }
        if let Some(running) = &self.running {
            running.cancel.cancel();
        }
        self.complete(true)?;
        for work in self.queue.drain(..) {
            if let Work::Request(request) = work {
                let _ = request.reply.send(Ok(
                    json!({"isError":true,"error":"CodeGraph service retired before admission"}),
                ));
            }
        }
        Ok(())
    }

    fn control(&mut self, request: &Request) -> io::Result<Value> {
        let id = request.payload["client"]
            .as_str()
            .filter(|v| v.len() == 64)
            .ok_or_else(|| io::Error::other("invalid CodeGraph client identity"))?;
        if request.name == "client/close" {
            self.clients.remove(id);
            return Ok(json!({"closed":true}));
        }
        if request.cancel.is_cancelled() || request.deadline.expired() {
            return Ok(json!({"isError":true,"error":"CodeGraph connection expired"}));
        }
        let root: PathBuf = serde_json::from_value(request.payload["project"].clone())?;
        if local_path(&root)? != root || !root.is_dir() {
            return Err(io::Error::other("CodeGraph client root must be canonical"));
        }
        if self.clients.len() >= CAPACITY
            || (!self.projects.contains_key(&root) && self.projects.len() >= 32)
        {
            return Ok(
                json!({"isError":true,"error":"CodeGraph active project/client allowance reached"}),
            );
        }
        let pid = request.payload["pid"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| io::Error::other("invalid CodeGraph client PID"))?;
        let created = request.payload["created"]
            .as_u64()
            .ok_or_else(|| io::Error::other("invalid CodeGraph client creation time"))?;
        let process = ServiceProcess::inspect(
            ProcessIdentity {
                pid,
                creation_time: created,
            },
            &std::env::current_exe()?,
            &current_user()?,
        )?
        .ok_or_else(|| io::Error::other("CodeGraph client is no longer alive"))?;
        if !self.projects.contains_key(&root) {
            let mut configuration = self.configuration.clone();
            configuration.project = root.clone();
            let runtime = Runtime::scheduled(configuration)?;
            let observer = if runtime.indexed()? {
                Some(Observer::start(&root)?)
            } else {
                None
            };
            self.projects.insert(
                root.clone(),
                Project {
                    runtime: Arc::new(Mutex::new(runtime)),
                    observer,
                    observed: 1,
                    committed: 0,
                    pending_since: Instant::now(),
                    last_event: Instant::now(),
                    queued: false,
                    last_client: None,
                    observation_error: None,
                    reconnect: false,
                },
            );
        }
        let project = self.projects.get_mut(&root).unwrap();
        if project.last_client.take().is_some() {
            project.reconnect = true;
            project.observation_error = None;
        }
        self.clients.insert(id.into(), Client { root, process });
        Ok(json!({"connected":true}))
    }

    fn observe(&mut self) -> io::Result<()> {
        let mut dead = Vec::new();
        for (id, client) in &self.clients {
            if !client.process.is_running()? {
                dead.push(id.clone());
            }
        }
        for id in dead {
            self.clients.remove(&id);
        }
        let mut retire = Vec::new();
        for (root, project) in &mut self.projects {
            if !self.clients.values().any(|client| &client.root == root) {
                let since = project.last_client.get_or_insert_with(Instant::now);
                // Stop observation immediately; retain only the light state until idle retirement.
                if let Some(mut observer) = project.observer.take() {
                    observer.close()?;
                }
                if since.elapsed() >= Duration::from_secs(60) {
                    retire.push(root.clone());
                }
                continue;
            }
            let runnable = if let Ok(mut runtime) = project.runtime.try_lock() {
                if project.reconnect {
                    runtime.reconnect();
                    project.reconnect = false;
                }
                runtime.retire_healthy()?;
                if project.observer.is_none()
                    && !runtime.failed()
                    && runtime.indexed()?
                    && project.observation_error.is_none()
                {
                    match Observer::start(root) {
                        Ok(observer) => {
                            project.observer = Some(observer);
                            project.committed = 0;
                            project.observed = 1;
                        }
                        Err(error) => {
                            project.observation_error = Some(error.to_string());
                        }
                    }
                }
                !runtime.failed()
            } else {
                false
            };
            if let Some(observer) = &project.observer {
                match observer.revision() {
                    Ok(revision) if revision != project.observed => {
                        if project.observed == project.committed {
                            project.pending_since = Instant::now();
                        }
                        project.observed = revision;
                        project.last_event = Instant::now();
                    }
                    Ok(_) => (),
                    Err(error) => {
                        project.observation_error = Some(error.to_string());
                    }
                }
            }
            if project.observer.is_some()
                && project.observation_error.is_none()
                && project.observed != project.committed
                && !project.queued
                && runnable
                && (project.last_event.elapsed() >= Duration::from_millis(500)
                    || project.pending_since.elapsed() >= Duration::from_secs(2))
                && self.queue.len() < CAPACITY
            {
                project.queued = true;
                self.queue.push_back(Work::Refresh(root.clone()));
            }
        }
        for root in retire {
            if let Some(project) = self.projects.remove(&root) {
                project
                    .runtime
                    .lock()
                    .map_err(|_| io::Error::other("CodeGraph runtime poisoned"))?
                    .close()?;
            }
            if self.active.as_ref() == Some(&root) {
                self.active = None;
            }
        }
        Ok(())
    }

    fn select(&mut self, root: &PathBuf) -> io::Result<()> {
        if self.active.as_ref() != Some(root) {
            if let Some(previous) = self.active.take()
                && let Some(project) = self.projects.get_mut(&previous)
            {
                project
                    .runtime
                    .lock()
                    .map_err(|_| io::Error::other("CodeGraph runtime poisoned"))?
                    .close()?;
            }
            self.active = Some(root.clone());
        }
        Ok(())
    }

    fn execute(&mut self, work: Work) -> io::Result<()> {
        let (root, name, arguments, deadline, caller, reply) = match work {
            Work::Request(request) => {
                if request
                    .stage
                    .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    return Ok(());
                }
                let Some(client) = request.payload["client"]
                    .as_str()
                    .and_then(|id| self.clients.get(id))
                else {
                    let _ = request.reply.send(Ok(
                        json!({"isError":true,"error":"CodeGraph client is not connected"}),
                    ));
                    return Ok(());
                };
                (
                    client.root.clone(),
                    request.name,
                    request.payload["arguments"].clone(),
                    request.deadline,
                    request.cancel,
                    Some(request.reply),
                )
            }
            Work::Refresh(root) => {
                let Some(project) = self.projects.get_mut(&root) else {
                    return Ok(());
                };
                project.queued = false;
                if project
                    .runtime
                    .lock()
                    .map_err(|_| io::Error::other("CodeGraph runtime poisoned"))?
                    .failed()
                    || project.last_client.is_some()
                    || project.observed == project.committed
                {
                    return Ok(());
                }
                (
                    root,
                    "codegraph_sync".into(),
                    json!({}),
                    Deadline::after(Duration::from_secs(600))?,
                    Cancellation::default(),
                    None,
                )
            }
        };
        if caller.is_cancelled() || deadline.expired() {
            if let Some(reply) = reply {
                let _ = reply.send(Ok(json!({"isError":true,"root":root,"freshness":"pending","error":"CodeGraph queued work cancelled or expired before admission"})));
            }
            return Ok(());
        }
        self.select(&root)?;
        let project = self.projects.get(&root).unwrap();
        let before = project
            .observer
            .as_ref()
            .and_then(|observer| observer.revision().ok())
            .unwrap_or(0);
        let runtime = project.runtime.clone();
        let cancel = Cancellation::default();
        let stopping = cancel.clone();
        let operation = name.clone();
        let handle = thread::Builder::new()
            .name("codegraph-episode".into())
            .spawn(move || {
                runtime
                    .lock()
                    .map_err(|_| io::Error::other("CodeGraph runtime poisoned"))?
                    .call(&operation, arguments, deadline, &stopping)
            })?;
        self.running = Some(Running {
            root,
            before,
            name,
            reply,
            caller,
            cancel,
            handle,
        });
        Ok(())
    }

    fn complete(&mut self, wait: bool) -> io::Result<()> {
        if self
            .running
            .as_ref()
            .is_none_or(|running| !wait && !running.handle.is_finished())
        {
            return Ok(());
        }
        let running = self.running.take().unwrap();
        let mut result = running
            .handle
            .join()
            .map_err(|_| io::Error::other("CodeGraph episode panicked"))??;
        let project = self
            .projects
            .get_mut(&running.root)
            .ok_or_else(|| io::Error::other("CodeGraph running project disappeared"))?;
        if matches!(running.name.as_str(), "codegraph_index" | "codegraph_sync")
            && result["isError"] != true
        {
            project.committed = running.before;
        }
        if let Some(observer) = &project.observer {
            let revision = observer.revision();
            if let Err(error) = &revision {
                project.observation_error = Some(error.to_string());
            }
            if revision.is_ok_and(|revision| revision != project.committed)
                && result["freshness"] != "failed"
            {
                result["freshness"] = json!("pending");
                result["source_fallback"] = json!(
                    "A source change is queued or occurred during the episode; use current Serena/source until automatic sync completes."
                );
            }
        }
        if let Some(error) = &project.observation_error {
            result["freshness"] = json!("failed");
            result["observation_error"] = json!(error);
        }
        if let Some(reply) = running.reply {
            let _ = reply.send(Ok(result));
        }
        Ok(())
    }
}
