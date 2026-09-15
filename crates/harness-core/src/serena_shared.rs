//! Bounded project-isolated shared Serena workers.
//!
//! Ports the seam's project pool: matching project/mode/configuration
//! selections share one serialized native worker; each client keeps its own
//! route and conversation state. Capacity evicts the least recently used
//! worker, dead or changed-configuration workers are replaced on demand, and
//! idle reaping closes workers and forgets idle clients. A client performing
//! remove_project becomes configuration-incompatible before the mutation.
#![cfg(windows)]

use crate::{
    process::{Deadline, ProcessIdentity},
    serena,
    serena_route::{self, Route},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    time::Instant,
};

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

const MAX_CLIENTS: usize = 128;

/// One owned native worker. Every call is serialized by the pool.
pub trait SharedWorker: Send {
    fn request(&mut self, method: &str, params: Value, deadline: Deadline) -> io::Result<Value>;
    fn initialized(&self) -> Value;
    fn identity(&self) -> ProcessIdentity;
    fn is_alive(&self) -> bool;
    /// Reclaim the owned tree; must return only after confirmed cleanup.
    fn close(&mut self) -> io::Result<()>;
}

pub type WorkerFactory =
    Box<dyn FnMut(&Route, &Value, Deadline) -> io::Result<Box<dyn SharedWorker>> + Send>;

struct PoolEntry {
    worker: Box<dyn SharedWorker>,
    route: Route,
    key: String,
    used: Instant,
}

struct ClientState {
    route: Route,
    initialize: Value,
    known_projects: Vec<String>,
    removed_projects: Vec<String>,
    used: Instant,
}

/// The shared project pool. Dispatch is serialized by the owning broker; this
/// type is not internally synchronized.
pub struct Pool {
    policy: serena_route::Policy,
    home: PathBuf,
    factory: WorkerFactory,
    workers: BTreeMap<String, PoolEntry>,
    clients: BTreeMap<String, ClientState>,
    used: Instant,
}

fn valid_client(client: &str) -> bool {
    client.len() == 32 && client.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl Pool {
    pub fn new(
        policy: serena_route::Policy,
        home: PathBuf,
        factory: WorkerFactory,
    ) -> io::Result<Self> {
        if !home.is_dir() {
            return Err(invalid("Serena shared home is unavailable"));
        }
        Ok(Self {
            policy,
            home,
            factory,
            workers: BTreeMap::new(),
            clients: BTreeMap::new(),
            used: Instant::now(),
        })
    }

    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// No connected client can still use a worker. Idle clients are dropped
    /// by the reaper, so a crashed proxy cannot pin the broker forever.
    pub fn is_idle(&self) -> bool {
        self.clients.is_empty()
    }

    /// Endpoint liveness view; membership is snapshotted before inspection so
    /// a starting or busy worker never blocks status.
    pub fn status(&self) -> Value {
        json!({
            "clients": self.clients.len(),
            "workers": self.workers.values().map(|entry| json!({
                "pid": entry.worker.identity().pid,
                "project": entry.route.project,
                "key": entry.key,
            })).collect::<Vec<_>>(),
        })
    }

    fn worker_for(
        &mut self,
        route: &Route,
        initialize: &Value,
        deadline: Deadline,
    ) -> io::Result<String> {
        let key = serena_route::configuration_key(route, initialize, &self.home)?;
        if let Some(entry) = self.workers.get_mut(&key)
            && entry.worker.is_alive()
        {
            entry.used = Instant::now();
            self.used = entry.used;
            return Ok(key);
        }
        // Retire dead workers and changed configuration for this selection
        // before replacement.
        let retired: Vec<String> = self
            .workers
            .iter()
            .filter(|(_, entry)| {
                !entry.worker.is_alive() || (&entry.route == route && entry.key != key)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in retired {
            if let Some(mut entry) = self.workers.remove(&key) {
                entry.worker.close()?;
            }
        }
        while self.workers.len() >= self.policy.max_projects {
            let oldest = self
                .workers
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
                .expect("capacity exceeds worker count");
            if let Some(mut entry) = self.workers.remove(&oldest) {
                entry.worker.close()?;
            }
        }
        let worker = (self.factory)(route, initialize, deadline)?;
        // Native startup may create project metadata or register the project;
        // the identity is recomputed after the worker exists.
        let key = serena_route::configuration_key(route, initialize, &self.home)?;
        let used = Instant::now();
        self.used = used;
        self.workers.insert(
            key.clone(),
            PoolEntry {
                worker,
                route: route.clone(),
                key: key.clone(),
                used,
            },
        );
        Ok(key)
    }

    /// Register a client from its original invocation. Returns the cached
    /// initialize result and the resolved route.
    pub fn connect(
        &mut self,
        client: &str,
        arguments: &[OsString],
        cwd: &Path,
        initialize: Value,
        deadline: Deadline,
    ) -> io::Result<Value> {
        if !valid_client(client) {
            return Err(invalid("Invalid Serena client identity"));
        }
        if !self.clients.contains_key(client) && self.clients.len() >= MAX_CLIENTS {
            return Err(io::Error::other("Serena client capacity reached"));
        }
        let route = serena_route::parse_route(arguments, cwd, &self.home)?;
        if let Some(project) = &route.project {
            let mut known = Vec::new();
            let value = project.to_string_lossy().into_owned();
            if !known.contains(&value) {
                known.push(value);
            }
            self.clients
                .entry(client.to_owned())
                .or_insert(ClientState {
                    route: route.clone(),
                    initialize: initialize.clone(),
                    known_projects: known,
                    removed_projects: Vec::new(),
                    used: Instant::now(),
                });
        } else {
            self.clients
                .entry(client.to_owned())
                .or_insert(ClientState {
                    route: route.clone(),
                    initialize: initialize.clone(),
                    known_projects: Vec::new(),
                    removed_projects: Vec::new(),
                    used: Instant::now(),
                });
        }
        let (route, initialize) = {
            let state = self.clients.get_mut(client).expect("client was registered");
            (state.route.clone(), state.initialize.clone())
        };
        let key = self.worker_for(&route, &initialize, deadline)?;
        let message = self
            .workers
            .get(&key)
            .expect("worker was started")
            .worker
            .initialized();
        Ok(json!({"message": message, "route": route.to_json()}))
    }

    /// Forward one client RPC. Unknown clients re-register from the supplied
    /// cached route, matching a proxy that outlived broker state.
    pub fn rpc(
        &mut self,
        client: &str,
        method: &str,
        params: Value,
        route: Route,
        initialize: Value,
        deadline: Deadline,
    ) -> io::Result<Value> {
        if !valid_client(client) {
            return Err(invalid("Invalid Serena client identity"));
        }
        if !self.clients.contains_key(client) {
            if self.clients.len() >= MAX_CLIENTS {
                return Err(io::Error::other("Serena client capacity reached"));
            }
            let removed = route.removed_projects.clone();
            self.clients.insert(
                client.to_owned(),
                ClientState {
                    route,
                    initialize,
                    known_projects: Vec::new(),
                    removed_projects: removed,
                    used: Instant::now(),
                },
            );
        }
        let state = self.clients.get_mut(client).expect("client is registered");
        state.used = Instant::now();
        self.used = state.used;
        let mut route = state.route.clone();
        let initialize = state.initialize.clone();
        let mut params = params;
        let activation = method == "tools/call" && params["name"] == "activate_project";
        let removal = method == "tools/call" && params["name"] == "remove_project";
        if activation {
            let selection = params["arguments"]["project"]
                .as_str()
                .ok_or_else(|| invalid("activate_project requires a project"))?
                .to_owned();
            let (known, removed) = {
                let state = self.clients.get(client).expect("client is registered");
                (state.known_projects.clone(), state.removed_projects.clone())
            };
            let resolved = serena_route::canonical_project(
                &selection, &route.cwd, &self.home, &known, &removed,
            )?;
            route.project = Some(resolved.clone());
            params["arguments"]["project"] = json!(resolved);
        }
        if removal {
            route.mutation_owner = Some(client.to_owned());
        }
        if method == "tools/call" {
            let meta = params.get("_meta").cloned().unwrap_or_else(|| json!({}));
            let mut meta = meta;
            meta["harness_serena_client"] = json!(client);
            params["_meta"] = meta;
        }
        let removed_name = (method == "tools/call" && params["name"] == "remove_project")
            .then(|| {
                params["arguments"]["project_name"]
                    .as_str()
                    .map(str::to_owned)
            })
            .flatten();
        let key = self.worker_for(&route, &initialize, deadline)?;
        let entry = self.workers.get_mut(&key).expect("worker was started");
        let result = entry.worker.request(method, params, deadline)?;
        let succeeded = result.get("error").is_none()
            && !result["result"]
                .get("isError")
                .is_some_and(|value| value != false);
        if activation && succeeded {
            let state = self.clients.get_mut(client).expect("client is registered");
            if let Some(project) = &route.project {
                let value = project.to_string_lossy().into_owned();
                if !state.known_projects.contains(&value) {
                    state.known_projects.push(value);
                }
            }
            state.route = route.clone();
        }
        if removal && succeeded {
            let state = self.clients.get_mut(client).expect("client is registered");
            if let Some(name) = removed_name
                && !state.route.removed_projects.contains(&name)
            {
                state.route.removed_projects.push(name);
            }
            route.removed_projects = state.route.removed_projects.clone();
            state.route = route;
        }
        let state = self.clients.get(client).expect("client is registered");
        Ok(json!({
            "message": result,
            "route": state.route.to_json(),
            "tools_changed": activation && succeeded,
        }))
    }

    pub fn disconnect(&mut self, client: &str) -> bool {
        self.clients.remove(client).is_some()
    }

    /// Close idle or dead workers and forget idle clients.
    pub fn reap(&mut self) {
        let idle = self.policy.idle_seconds;
        let now = Instant::now();
        let idle_workers: Vec<String> = self
            .workers
            .iter()
            .filter(|(_, entry)| {
                !entry.worker.is_alive() || now.duration_since(entry.used).as_secs() >= idle
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in idle_workers {
            if let Some(mut entry) = self.workers.remove(&key) {
                let _ = entry.worker.close();
            }
        }
        self.clients
            .retain(|_, state| now.duration_since(state.used).as_secs() < idle);
    }

    pub fn close(&mut self) -> io::Result<()> {
        let mut failure = None;
        for (_, mut entry) in std::mem::take(&mut self.workers) {
            if let Err(error) = entry.worker.close() {
                failure = Some(error);
            }
        }
        self.clients.clear();
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// The real worker factory around the guarded shared entry point.
pub fn session_factory(
    launch: serena::Launch,
    home: PathBuf,
    cancel: crate::process::Cancellation,
) -> io::Result<WorkerFactory> {
    let stderr_root = launch.home.join("harness/runtime/serena-workers");
    std::fs::create_dir_all(&stderr_root)?;
    let serial = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    Ok(Box::new(move |route, initialize, deadline| {
        let command = serena::shared_command(&launch, route, &route.removed_projects, &home)?;
        let index = serial.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let stderr = stderr_root.join(format!("worker-{index}.txt"));
        let mut session = serena::Session::start_shared(command, stderr, &cancel)?;
        session.initialize_shared(initialize.clone(), deadline)?;
        Ok(Box::new(SerenaWorker {
            session: Some(session),
        }) as Box<dyn SharedWorker>)
    }))
}

struct SerenaWorker {
    session: Option<serena::Session>,
}

impl SharedWorker for SerenaWorker {
    fn request(&mut self, method: &str, params: Value, deadline: Deadline) -> io::Result<Value> {
        self.session
            .as_mut()
            .ok_or_else(|| io::Error::other("Serena shared worker is closed"))?
            .request(method, params, deadline)
    }

    fn initialized(&self) -> Value {
        self.session
            .as_ref()
            .map(|session| session.initialized_result())
            .unwrap_or(Value::Null)
    }

    fn identity(&self) -> ProcessIdentity {
        self.session
            .as_ref()
            .map(serena::Session::identity)
            .expect("worker identity requires a live session")
    }

    fn is_alive(&self) -> bool {
        self.session.as_ref().is_some_and(serena::Session::is_alive)
    }

    fn close(&mut self) -> io::Result<()> {
        if let Some(session) = self.session.take() {
            let outcome = session.close()?;
            if !matches!(
                outcome.reason,
                crate::process::StopReason::Exited | crate::process::StopReason::Cancelled
            ) {
                return Err(io::Error::other(
                    "Serena shared worker tree cleanup was not confirmed",
                ));
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::Deadline;
    use serde_json::json;
    use std::{
        fs,
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };

    #[derive(Default)]
    struct FakeState {
        alive: bool,
        requests: Vec<(String, Value)>,
        closed: bool,
    }

    struct FakeWorker {
        identity: ProcessIdentity,
        initialized: Value,
        state: Arc<Mutex<FakeState>>,
    }

    impl SharedWorker for FakeWorker {
        fn request(
            &mut self,
            method: &str,
            params: Value,
            deadline: Deadline,
        ) -> io::Result<Value> {
            let mut state = self.state.lock().unwrap();
            state.requests.push((method.to_owned(), params.clone()));
            if deadline.expired() || !state.alive {
                return Err(io::Error::other("fixture worker is unavailable"));
            }
            Ok(json!({
                "jsonrpc": "2.0",
                "id": 7,
                "result": {"content": [], "structuredContent": params},
            }))
        }

        fn initialized(&self) -> Value {
            json!({"jsonrpc": "2.0", "id": 1, "result": self.initialized})
        }

        fn identity(&self) -> ProcessIdentity {
            self.identity
        }

        fn is_alive(&self) -> bool {
            self.state.lock().unwrap().alive
        }

        fn close(&mut self) -> io::Result<()> {
            let mut state = self.state.lock().unwrap();
            state.alive = false;
            state.closed = true;
            Ok(())
        }
    }

    struct Fixture {
        _root: tempfile::TempDir,
        home: PathBuf,
        starts: Arc<AtomicU64>,
        states: Arc<Mutex<Vec<Arc<Mutex<FakeState>>>>>,
    }

    impl Fixture {
        fn new(tag: &str, max_projects: usize, idle_seconds: u64) -> (Self, Pool) {
            let root = tempfile::Builder::new()
                .prefix(&format!("harness-serena-pool-{tag}-"))
                .tempdir()
                .unwrap();
            let home = root.path().join("serena-home");
            fs::create_dir_all(home.join("contexts")).unwrap();
            fs::write(home.join("serena_config.yml"), "projects: []\n").unwrap();
            fs::write(home.join("contexts/codex.yml"), "tools: []\n").unwrap();
            let starts = Arc::new(AtomicU64::new(0));
            let states: Arc<Mutex<Vec<Arc<Mutex<FakeState>>>>> = Arc::new(Mutex::new(Vec::new()));
            let factory_starts = Arc::clone(&starts);
            let factory_states = Arc::clone(&states);
            let factory: WorkerFactory = Box::new(move |_route, _initialize, _deadline| {
                let index = factory_starts.fetch_add(1, Ordering::SeqCst);
                let state = Arc::new(Mutex::new(FakeState {
                    alive: true,
                    requests: Vec::new(),
                    closed: false,
                }));
                factory_states.lock().unwrap().push(Arc::clone(&state));
                Ok(Box::new(FakeWorker {
                    identity: ProcessIdentity {
                        pid: 1000 + index as u32,
                        creation_time: index,
                    },
                    initialized: json!({"serverInfo": {"name": "Serena"}, "fixture": index}),
                    state,
                }) as Box<dyn SharedWorker>)
            });
            let pool = Pool::new(
                serena_route::Policy {
                    max_projects,
                    idle_seconds,
                },
                home.clone(),
                factory,
            )
            .unwrap();
            (
                Self {
                    _root: root,
                    home,
                    starts,
                    states,
                },
                pool,
            )
        }

        fn project(&self, name: &str) -> PathBuf {
            let project = self.home.parent().unwrap().join(name);
            fs::create_dir_all(project.join(".serena")).unwrap();
            project
        }

        fn client(id: usize) -> String {
            format!("{:032x}", id)
        }

        fn deadline() -> Deadline {
            Deadline::after(Duration::from_secs(30)).unwrap()
        }
    }

    fn managed_arguments(project: &Path) -> Vec<OsString> {
        vec![
            "start-mcp-server".into(),
            "--project".into(),
            project.as_os_str().to_os_string(),
            "--context".into(),
            "codex".into(),
        ]
    }

    #[test]
    fn same_selection_clients_share_one_worker() {
        let (fixture, mut pool) = Fixture::new("share", 3, 300);
        let project = fixture.project("alpha");
        let first = pool
            .connect(
                &Fixture::client(1),
                &managed_arguments(&project),
                &project,
                json!({"protocolVersion": "2024-11-05"}),
                Fixture::deadline(),
            )
            .unwrap();
        assert_eq!(first["message"]["result"]["serverInfo"]["name"], "Serena");
        let second = pool
            .connect(
                &Fixture::client(2),
                &managed_arguments(&project),
                &project,
                json!({"protocolVersion": "2024-11-05"}),
                Fixture::deadline(),
            )
            .unwrap();
        assert_eq!(
            second["message"]["result"]["fixture"],
            first["message"]["result"]["fixture"]
        );
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        assert_eq!(pool.worker_count(), 1);
        assert_eq!(pool.client_count(), 2);
        let status = pool.status();
        assert_eq!(status["clients"], 2);
        assert_eq!(status["workers"].as_array().unwrap().len(), 1);
        pool.close().unwrap();
        assert!(
            fixture
                .states
                .lock()
                .unwrap()
                .iter()
                .all(|state| state.lock().unwrap().closed)
        );
    }

    #[test]
    fn capacity_evicts_the_least_recently_used_worker() {
        let (fixture, mut pool) = Fixture::new("capacity", 2, 300);
        let first = fixture.project("one");
        let second = fixture.project("two");
        let third = fixture.project("three");
        for (index, project) in [&first, &second, &third].into_iter().enumerate() {
            pool.connect(
                &Fixture::client(index + 1),
                &managed_arguments(project),
                project,
                json!({"protocolVersion": "2024-11-05"}),
                Fixture::deadline(),
            )
            .unwrap();
        }
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 3);
        assert_eq!(pool.worker_count(), 2);
        let closed: Vec<bool> = fixture
            .states
            .lock()
            .unwrap()
            .iter()
            .map(|state| state.lock().unwrap().closed)
            .collect();
        assert!(closed.contains(&true) && closed.contains(&false));
        assert_eq!(closed.iter().filter(|closed| **closed).count(), 1);
        pool.close().unwrap();
    }

    #[test]
    fn dead_worker_is_replaced_on_the_next_request() {
        let (fixture, mut pool) = Fixture::new("dead", 3, 300);
        let project = fixture.project("dead-project");
        let client = Fixture::client(7);
        pool.connect(
            &client,
            &managed_arguments(&project),
            &project,
            json!({"protocolVersion": "2024-11-05"}),
            Fixture::deadline(),
        )
        .unwrap();
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        {
            let states = fixture.states.lock().unwrap();
            states[0].lock().unwrap().alive = false;
        }
        let state = crate::serena_route::Route {
            project: Some(project.clone()),
            cwd: project.clone(),
            arguments: vec![
                "start-mcp-server".into(),
                "--context".into(),
                "codex".into(),
            ],
            removed_projects: Vec::new(),
            mutation_owner: None,
        };
        let result = pool
            .rpc(
                &client,
                "tools/list",
                json!({}),
                state,
                json!({"protocolVersion": "2024-11-05"}),
                Fixture::deadline(),
            )
            .unwrap();
        assert!(
            result["message"]["result"]
                .get("structuredContent")
                .is_some()
        );
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 2);
        assert!(fixture.states.lock().unwrap()[0].lock().unwrap().closed);
        pool.close().unwrap();
    }

    #[test]
    fn configuration_change_replaces_the_worker_for_that_selection() {
        let (fixture, mut pool) = Fixture::new("config", 3, 300);
        let project = fixture.project("configured");
        let client = Fixture::client(9);
        pool.connect(
            &client,
            &managed_arguments(&project),
            &project,
            json!({"protocolVersion": "2024-11-05"}),
            Fixture::deadline(),
        )
        .unwrap();
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        fs::write(
            fixture.home.join("contexts/codex.yml"),
            "tools:\n  - find_symbol\n",
        )
        .unwrap();
        let route = crate::serena_route::Route {
            project: Some(project.clone()),
            cwd: project.clone(),
            arguments: vec![
                "start-mcp-server".into(),
                "--context".into(),
                "codex".into(),
            ],
            removed_projects: Vec::new(),
            mutation_owner: None,
        };
        pool.rpc(
            &client,
            "tools/list",
            json!({}),
            route,
            json!({"protocolVersion": "2024-11-05"}),
            Fixture::deadline(),
        )
        .unwrap();
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 2);
        assert!(fixture.states.lock().unwrap()[0].lock().unwrap().closed);
        pool.close().unwrap();
    }

    #[test]
    fn activate_project_updates_the_route_and_reports_tools_changed() {
        let (fixture, mut pool) = Fixture::new("activate", 3, 300);
        let alpha = fixture.project("alpha-route");
        let beta = fixture.project("beta-route");
        let client = Fixture::client(11);
        let connected = pool
            .connect(
                &client,
                &managed_arguments(&alpha),
                &alpha,
                json!({"protocolVersion": "2024-11-05"}),
                Fixture::deadline(),
            )
            .unwrap();
        assert_eq!(connected["route"]["project"], json!(alpha));
        let result = pool
            .rpc(
                &client,
                "tools/call",
                json!({"name": "activate_project", "arguments": {"project": beta.to_string_lossy()}}),
                crate::serena_route::Route {
                    project: Some(alpha.clone()),
                    cwd: alpha.clone(),
                    arguments: vec![
                        "start-mcp-server".into(),
                        "--context".into(),
                        "codex".into(),
                    ],
                    removed_projects: Vec::new(),
                    mutation_owner: None,
                },
                json!({"protocolVersion": "2024-11-05"}),
                Fixture::deadline(),
            )
            .unwrap();
        assert_eq!(result["tools_changed"], true);
        assert_eq!(result["route"]["project"], json!(beta));
        let requests: Vec<(String, Value)> = fixture
            .states
            .lock()
            .unwrap()
            .iter()
            .flat_map(|state| state.lock().unwrap().requests.clone())
            .collect();
        let activation = requests
            .iter()
            .find(|(method, _)| method == "tools/call")
            .unwrap();
        assert_eq!(
            activation.1["_meta"]["harness_serena_client"],
            json!(client)
        );
        assert_eq!(activation.1["arguments"]["project"], json!(beta));
        pool.close().unwrap();
    }

    #[test]
    fn remove_project_marks_the_client_incompatible_before_mutation() {
        let (fixture, mut pool) = Fixture::new("remove", 3, 300);
        let project = fixture.project("removal");
        let client = Fixture::client(13);
        pool.connect(
            &client,
            &managed_arguments(&project),
            &project,
            json!({"protocolVersion": "2024-11-05"}),
            Fixture::deadline(),
        )
        .unwrap();
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        let route = crate::serena_route::Route {
            project: Some(project.clone()),
            cwd: project.clone(),
            arguments: vec![
                "start-mcp-server".into(),
                "--context".into(),
                "codex".into(),
            ],
            removed_projects: Vec::new(),
            mutation_owner: None,
        };
        let result = pool
            .rpc(
                &client,
                "tools/call",
                json!({"name": "remove_project", "arguments": {"project_name": "legacy"}}),
                route,
                json!({"protocolVersion": "2024-11-05"}),
                Fixture::deadline(),
            )
            .unwrap();
        assert_eq!(result["route"]["removed_projects"], json!(["legacy"]));
        assert_eq!(result["route"]["mutation_owner"], json!(client));
        // The mutating client needs a fresh worker for its changed identity.
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 2);
        pool.close().unwrap();
    }

    #[test]
    fn client_identity_and_capacity_rules_are_enforced() {
        let (fixture, mut pool) = Fixture::new("clients", 3, 300);
        let project = fixture.project("clients-project");
        assert!(
            pool.connect(
                "short",
                &managed_arguments(&project),
                &project,
                json!({"protocolVersion": "2024-11-05"}),
                Fixture::deadline(),
            )
            .is_err()
        );
        for index in 0..128 {
            pool.connect(
                &Fixture::client(index + 1),
                &managed_arguments(&project),
                &project,
                json!({"protocolVersion": "2024-11-05"}),
                Fixture::deadline(),
            )
            .unwrap();
        }
        assert!(
            pool.connect(
                &Fixture::client(200),
                &managed_arguments(&project),
                &project,
                json!({"protocolVersion": "2024-11-05"}),
                Fixture::deadline(),
            )
            .is_err()
        );
        assert_eq!(pool.client_count(), 128);
        assert!(pool.disconnect(&Fixture::client(1)));
        assert!(!pool.disconnect(&Fixture::client(1)));
        pool.connect(
            &Fixture::client(200),
            &managed_arguments(&project),
            &project,
            json!({"protocolVersion": "2024-11-05"}),
            Fixture::deadline(),
        )
        .unwrap();
        pool.close().unwrap();
    }

    #[test]
    fn unknown_rpc_client_registers_from_its_cached_route() {
        let (fixture, mut pool) = Fixture::new("unknown", 3, 300);
        let project = fixture.project("unknown-project");
        let client = Fixture::client(21);
        let result = pool
            .rpc(
                &client,
                "tools/list",
                json!({}),
                crate::serena_route::Route {
                    project: Some(project.clone()),
                    cwd: project.clone(),
                    arguments: vec![
                        "start-mcp-server".into(),
                        "--context".into(),
                        "codex".into(),
                    ],
                    removed_projects: Vec::new(),
                    mutation_owner: None,
                },
                json!({"protocolVersion": "2024-11-05"}),
                Fixture::deadline(),
            )
            .unwrap();
        assert!(
            result["message"]["result"]
                .get("structuredContent")
                .is_some()
        );
        assert_eq!(pool.client_count(), 1);
        pool.close().unwrap();
    }

    #[test]
    fn reap_closes_idle_workers_and_forgets_idle_clients() {
        let (fixture, mut pool) = Fixture::new("reap", 3, 1);
        let project = fixture.project("idle");
        let client = Fixture::client(31);
        pool.connect(
            &client,
            &managed_arguments(&project),
            &project,
            json!({"protocolVersion": "2024-11-05"}),
            Fixture::deadline(),
        )
        .unwrap();
        assert_eq!(pool.worker_count(), 1);
        std::thread::sleep(Duration::from_millis(1100));
        pool.reap();
        assert_eq!(pool.worker_count(), 0);
        assert_eq!(pool.client_count(), 0);
        assert!(fixture.states.lock().unwrap()[0].lock().unwrap().closed);
        pool.close().unwrap();
    }
}
