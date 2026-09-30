//! Bounded independent Serena workers.
//!
//! Matching project/mode/configuration selections share one serialized native
//! worker; each client keeps its own route and conversation state. The table
//! lock only reserves worker generations and applies per-client routing
//! transitions: worker startup, requests and retirement run outside it.
//! Requests using one worker stay serialized, one client's mutable routing
//! stays ordered, configured capacity counts active, starting and retiring
//! generations until ownership is released, and only idle generations may be
//! replaced. A worker whose language-server manager failed during project
//! initialization is retired and the same call is retried once on a fresh
//! worker; no other call is retried automatically.
#![cfg(windows)]

use crate::{
    process::{Cancellation, Deadline, ProcessIdentity},
    serena,
    serena_route::{self, Route},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, TryLockError,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

const MAX_CLIENTS: usize = 128;
/// Capacity and startup waiters re-check their deadline, cancellation and the
/// pool state after every slice.
const WAIT_SLICE: Duration = Duration::from_millis(50);
/// Poll interval while another request holds a worker slot.
const GATE_POLL: Duration = Duration::from_millis(2);

/// One owned native worker. Requests through one slot are serialized; the
/// worker itself is closed only after its confirmed ownership release.
pub trait SharedWorker: Send {
    fn request(&mut self, method: &str, params: Value, deadline: Deadline) -> io::Result<Value>;
    fn initialized(&self) -> Value;
    fn identity(&self) -> ProcessIdentity;
    fn is_alive(&self) -> bool;
    /// Reclaim the owned tree; must return only after confirmed cleanup.
    fn close(&mut self) -> io::Result<()>;
}

/// Creates one worker per compatible configuration. A shared worker start may
/// run on any request thread, so the factory is reentrant and side-effect free
/// beyond its own bounded process ownership.
pub type WorkerFactory =
    Box<dyn Fn(&Route, &Value, Deadline) -> io::Result<Box<dyn SharedWorker>> + Send + Sync>;

/// Serena's own fatal marker for a language-server manager that failed during
/// project initialization. The pool replaces that worker and retries the call
/// once; a second failure is returned to the client.
fn fatal_language_server(result: &Value) -> bool {
    result["result"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["text"].as_str())
        .any(|text| text.contains("The language server manager is not initialized"))
}

fn valid_client(client: &str) -> bool {
    client.len() == 32 && client.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

fn interrupted(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, format!("{what} cancelled"))
}

fn timed_out(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, format!("{what} deadline elapsed"))
}

/// Take a lock within the caller's deadline: a slow owner releases the gate
/// when its own bounded request finishes, and an expiring waiter never
/// executes its operation afterwards.
fn lock_until<'a, T>(
    mutex: &'a Mutex<T>,
    deadline: Deadline,
    cancel: &Cancellation,
    what: &str,
) -> io::Result<MutexGuard<'a, T>> {
    loop {
        if cancel.is_cancelled() {
            return Err(interrupted(what));
        }
        if deadline.expired() {
            return Err(timed_out(what));
        }
        match mutex.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => {
                return Err(io::Error::other(format!("{what} is poisoned")));
            }
            Err(TryLockError::WouldBlock) => std::thread::sleep(GATE_POLL),
        }
    }
}

/// One worker generation. `used_ms` and `reservations` are read and written
/// while the pool table lock is held; `gate` serializes requests into the
/// worker and owns the worker box until its confirmed close.
struct Slot {
    generation: u64,
    key: String,
    route: Route,
    identity: ProcessIdentity,
    gate: Mutex<Option<Box<dyn SharedWorker>>>,
    /// Set when this generation was removed from the table and counted as
    /// retiring; the current gate holder or any closer takes and closes it.
    dead: AtomicBool,
    /// Milliseconds since the pool epoch of the last reservation or release.
    used_ms: AtomicU64,
    reservations: AtomicUsize,
}

struct ClientState {
    route: Route,
    initialize: Value,
    known_projects: Vec<String>,
    removed_projects: Vec<String>,
    used: Instant,
}

/// One client's routing state. The mutex is the client's ordering gate: one
/// client's mutable routing transitions and its worker interaction stay
/// ordered, while independent clients proceed concurrently.
struct ClientEntry {
    state: Mutex<ClientState>,
}

/// A compatible cold start being shared by every concurrent caller.
struct Startup {
    serial: u64,
    waiters: usize,
}

/// A shared startup outcome retained until the callers that waited for it
/// observed the failure; it owns no worker and is not counted against
/// capacity.
struct FailedStartup {
    message: String,
    waiters: usize,
}

/// Bounded duration evidence: counts and millisecond totals never grow in
/// shape, only in saturating values.
#[derive(Default)]
struct Durations {
    count: u64,
    total_ms: u64,
    max_ms: u64,
}

impl Durations {
    fn record(&mut self, elapsed: Duration) {
        let elapsed = millis(elapsed);
        self.count += 1;
        self.total_ms = self.total_ms.saturating_add(elapsed);
        self.max_ms = self.max_ms.max(elapsed);
    }

    fn to_json(&self) -> Value {
        json!({"count": self.count, "total_ms": self.total_ms, "max_ms": self.max_ms})
    }
}

struct Table {
    /// Ready generations keyed by configuration identity.
    workers: BTreeMap<String, Arc<Slot>>,
    /// Cold starts currently running or awaiting their waiters.
    starting: BTreeMap<String, Startup>,
    /// Failed shared starts that waiters still have to observe.
    failed: BTreeMap<u64, FailedStartup>,
    clients: BTreeMap<String, Arc<ClientEntry>>,
    /// Generations that left the table but have not released ownership yet.
    retiring: usize,
    serial: u64,
    closed: bool,
    hits: u64,
    cold_starts: u64,
    evictions: u64,
    startup_failures: u64,
    serialized_startups: u64,
    queue: Durations,
    start: Durations,
    request: Durations,
}

impl Table {
    /// Capacity covers every generation whose ownership is not released:
    /// active, starting and retiring workers.
    fn generations(&self) -> usize {
        self.workers.len() + self.starting.len() + self.retiring
    }

    fn take_startup(&mut self, key: &str, serial: u64) -> usize {
        match self.starting.get(key) {
            Some(startup) if startup.serial == serial => {
                self.starting.remove(key).expect("checked").waiters
            }
            _ => 0,
        }
    }
}

/// The internally synchronized shared project pool. Every public operation is
/// callable from any request thread.
pub struct Pool {
    policy: serena_route::Policy,
    home: PathBuf,
    factory: WorkerFactory,
    table: Mutex<Table>,
    changed: Condvar,
    /// Monotonic origin for every slot's `used_ms` evidence.
    epoch: Instant,
    identity: String,
    started_unix_ms: u64,
}

/// A reserved worker generation. The reservation keeps this generation out of
/// idle eviction until its owner releases it.
struct Lease {
    slot: Arc<Slot>,
}

/// One reserved shared startup, from its table entry to its configuration.
struct StartupTicket<'a> {
    serial: u64,
    key: &'a str,
    route: &'a Route,
    initialize: &'a Value,
}

/// Everything needed to publish one finished startup generation.
struct Publication<'a> {
    serial: u64,
    key: &'a str,
    route: &'a Route,
    identity: ProcessIdentity,
    worker: Box<dyn SharedWorker>,
    used_ms: u64,
    queue_wait: Duration,
}

enum Step {
    Use(Arc<Slot>),
    Retire(Vec<Arc<Slot>>),
    Begin(u64),
    Wait,
    Failed(String),
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
            table: Mutex::new(Table {
                workers: BTreeMap::new(),
                starting: BTreeMap::new(),
                failed: BTreeMap::new(),
                clients: BTreeMap::new(),
                retiring: 0,
                serial: 0,
                closed: false,
                hits: 0,
                cold_starts: 0,
                evictions: 0,
                startup_failures: 0,
                serialized_startups: 0,
                queue: Durations::default(),
                start: Durations::default(),
                request: Durations::default(),
            }),
            changed: Condvar::new(),
            epoch: Instant::now(),
            identity: crate::broker_endpoint::random_key()?,
            started_unix_ms: unix_ms_now(),
        })
    }

    fn lock_table(&self) -> io::Result<MutexGuard<'_, Table>> {
        self.table
            .lock()
            .map_err(|_| io::Error::other("Serena pool table lock poisoned"))
    }

    fn now_ms(&self) -> u64 {
        millis(self.epoch.elapsed())
    }

    pub fn worker_count(&self) -> usize {
        self.lock_table()
            .map(|table| table.workers.len())
            .unwrap_or(0)
    }

    pub fn client_count(&self) -> usize {
        self.lock_table()
            .map(|table| table.clients.len())
            .unwrap_or(0)
    }

    /// No connected client can still use a worker. Idle clients are dropped by
    /// the reaper, so a crashed proxy cannot pin the broker forever.
    pub fn is_idle(&self) -> bool {
        self.lock_table()
            .map(|table| table.clients.is_empty())
            .unwrap_or(false)
    }

    /// Endpoint liveness view: bounded counters, counts, durations and reset
    /// identity. Memory observation is explicitly unavailable; only the
    /// configured per-worker Job limit is known.
    pub fn status(&self) -> Value {
        let Ok(table) = self.lock_table() else {
            return json!({"poisoned": true});
        };
        let now_ms = self.now_ms();
        let reserved = table
            .workers
            .values()
            .filter(|slot| slot.reservations.load(Ordering::Acquire) > 0)
            .count();
        json!({
            "clients": table.clients.len(),
            "workers": table.workers.values().map(|slot| json!({
                "pid": slot.identity.pid,
                "project": slot.route.project,
                "key": slot.key,
                "generation": slot.generation,
                "reserved": slot.reservations.load(Ordering::Acquire) > 0,
                "idle_ms": now_ms.saturating_sub(slot.used_ms.load(Ordering::Acquire)),
            })).collect::<Vec<_>>(),
            "counts": {
                "active": table.workers.len(),
                "reserved": reserved,
                "idle": table.workers.len() - reserved,
                "starting": table.starting.len(),
                "retiring": table.retiring,
                "limit": self.policy.max_projects,
                "concurrent_startup_limit": self.policy.max_concurrent_startups,
            },
            "counters": {
                "hits": table.hits,
                "cold_starts": table.cold_starts,
                "evictions": table.evictions,
                "startup_failures": table.startup_failures,
                "serialized_startups": table.serialized_startups,
            },
            "durations": {
                "queue": table.queue.to_json(),
                "start": table.start.to_json(),
                "request": table.request.to_json(),
            },
            "interval": {
                "identity": self.identity,
                "started_unix_ms": self.started_unix_ms,
                "elapsed_ms": unix_ms_now().saturating_sub(self.started_unix_ms),
            },
            "memory": {
                "observation": "unavailable",
                "observed_bytes": Value::Null,
                "configured_limit_bytes": crate::serena::WORKER_JOB_MEMORY_BYTES,
            },
        })
    }

    /// Register a client from its original invocation (`connect`) or from its
    /// cached route (`rpc`) and return its ordering gate.
    fn client_entry(
        &self,
        client: &str,
        route: &Route,
        initialize: &Value,
        connect: bool,
    ) -> io::Result<Arc<ClientEntry>> {
        let mut table = self.lock_table()?;
        if let Some(entry) = table.clients.get(client) {
            return Ok(Arc::clone(entry));
        }
        if table.closed {
            return Err(io::Error::other("Serena pool is shutting down"));
        }
        if table.clients.len() >= MAX_CLIENTS {
            return Err(io::Error::other("Serena client capacity reached"));
        }
        let known_projects = if connect {
            route
                .project
                .iter()
                .map(|project| project.to_string_lossy().into_owned())
                .collect()
        } else {
            Vec::new()
        };
        let removed_projects = if connect {
            Vec::new()
        } else {
            route.removed_projects.clone()
        };
        let entry = Arc::new(ClientEntry {
            state: Mutex::new(ClientState {
                route: route.clone(),
                initialize: initialize.clone(),
                known_projects,
                removed_projects,
                used: Instant::now(),
            }),
        });
        table.clients.insert(client.to_owned(), Arc::clone(&entry));
        Ok(entry)
    }

    /// One short table decision. It never starts, requests or closes a worker;
    /// the caller performs those outside the lock.
    fn step(
        &self,
        table: &mut Table,
        key: &str,
        route: &Route,
        waiting: &mut Option<u64>,
    ) -> io::Result<Step> {
        if table.closed {
            return Err(io::Error::other("Serena pool is shutting down"));
        }
        if let Some(serial) = *waiting
            && table.starting.get(key).map(|startup| startup.serial) != Some(serial)
        {
            if let Some(failed) = table.failed.get(&serial) {
                return Ok(Step::Failed(failed.message.clone()));
            }
            *waiting = None;
        }
        if let Some(slot) = table.workers.get(key) {
            if slot.dead.load(Ordering::Acquire) {
                let slot = table.workers.remove(key).expect("checked");
                slot.reservations.store(0, Ordering::Release);
                table.retiring += 1;
                return Ok(Step::Retire(vec![slot]));
            }
            slot.reservations.fetch_add(1, Ordering::AcqRel);
            slot.used_ms.store(self.now_ms(), Ordering::Release);
            table.hits += 1;
            return Ok(Step::Use(Arc::clone(slot)));
        }
        if let Some(startup) = table.starting.get_mut(key) {
            if *waiting != Some(startup.serial) {
                startup.waiters += 1;
                *waiting = Some(startup.serial);
            }
            return Ok(Step::Wait);
        }
        // A generation of the same selection with an obsolete configuration
        // identity is replaced once it is idle; in-flight work is never taken.
        let obsolete: Vec<Arc<Slot>> = table
            .workers
            .iter()
            .filter(|(other, slot)| {
                other.as_str() != key
                    && slot.route == *route
                    && slot.reservations.load(Ordering::Acquire) == 0
            })
            .map(|(_, slot)| Arc::clone(slot))
            .collect();
        if !obsolete.is_empty() {
            for slot in &obsolete {
                table.workers.remove(&slot.key);
                slot.reservations.store(0, Ordering::Release);
                slot.dead.store(true, Ordering::Release);
                table.retiring += 1;
            }
            return Ok(Step::Retire(obsolete));
        }
        // Cold starts are bounded across configurations: one fresh worker
        // commits a whole language-server tree to the host, and a burst of
        // concurrent startups can exhaust the machine before any per-worker
        // job limit is reached. Callers of the same configuration keep
        // sharing one startup above; this only serializes distinct starts.
        if table.starting.len() >= self.policy.max_concurrent_startups {
            table.serialized_startups += 1;
            return Ok(Step::Wait);
        }
        if table.generations() < self.policy.max_projects {
            table.serial += 1;
            let serial = table.serial;
            table
                .starting
                .insert(key.to_owned(), Startup { serial, waiters: 0 });
            return Ok(Step::Begin(serial));
        }
        // Capacity is full: only an idle generation may be replaced, and the
        // replacement starts after its ownership is released.
        if let Some(victim) = table
            .workers
            .iter()
            .filter(|(_, slot)| slot.reservations.load(Ordering::Acquire) == 0)
            .min_by_key(|(_, slot)| slot.used_ms.load(Ordering::Acquire))
            .map(|(key, _)| key.clone())
        {
            let slot = table.workers.remove(&victim).expect("checked");
            slot.reservations.store(0, Ordering::Release);
            slot.dead.store(true, Ordering::Release);
            table.retiring += 1;
            table.evictions += 1;
            return Ok(Step::Retire(vec![slot]));
        }
        Ok(Step::Wait)
    }

    fn wait_slice(&self, deadline: Deadline, cancel: &Cancellation) -> io::Result<Duration> {
        if cancel.is_cancelled() {
            return Err(interrupted("Serena request"));
        }
        let slice = deadline.remaining().min(WAIT_SLICE);
        if slice.is_zero() {
            return Err(timed_out("Serena request"));
        }
        let started = Instant::now();
        let table = self.lock_table()?;
        let _ = self
            .changed
            .wait_timeout(table, slice)
            .map_err(|_| io::Error::other("Serena pool table lock poisoned"))?;
        Ok(started.elapsed())
    }

    fn record_queue(&self, elapsed: Duration) {
        if let Ok(mut table) = self.table.lock() {
            table.queue.record(elapsed);
        }
    }

    /// Release one reservation. A generation retired meanwhile no longer owns
    /// the reservation and needs no bookkeeping here.
    fn release(&self, slot: &Arc<Slot>) {
        if let Ok(table) = self.table.lock() {
            if let Some(entry) = table.workers.get(&slot.key)
                && Arc::ptr_eq(entry, slot)
            {
                let mut current = entry.reservations.load(Ordering::Acquire);
                while current > 0 {
                    match entry.reservations.compare_exchange(
                        current,
                        current - 1,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => break,
                        Err(actual) => current = actual,
                    }
                }
                entry.used_ms.store(self.now_ms(), Ordering::Release);
            }
            self.changed.notify_all();
        }
    }

    /// Reserve a worker generation for `route`, starting a shared cold worker
    /// when none is compatible. Waiting stays within the caller's deadline and
    /// cancellation; an expired request never reaches the worker.
    fn reserve(
        &self,
        route: &Route,
        initialize: &Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Lease> {
        let mut waiting: Option<u64> = None;
        let mut waited = Duration::ZERO;
        let outcome = (|| -> io::Result<Lease> {
            loop {
                if cancel.is_cancelled() {
                    return Err(interrupted("Serena request"));
                }
                if deadline.expired() {
                    return Err(timed_out("Serena request before admitting a worker"));
                }
                // The identity is recomputed every iteration: a shared startup
                // may register the project or create hashed configuration, and
                // an observed change must be honoured rather than reused.
                let key = serena_route::configuration_key(route, initialize, &self.home)?;
                let step = {
                    let mut table = self.lock_table()?;
                    self.step(&mut table, &key, route, &mut waiting)?
                };
                match step {
                    Step::Use(slot) => {
                        self.record_queue(waited);
                        return Ok(Lease { slot });
                    }
                    Step::Retire(slots) => {
                        for slot in slots {
                            self.close_ready(&slot);
                        }
                    }
                    Step::Begin(serial) => {
                        let started = Instant::now();
                        let outcome = (self.factory)(route, initialize, deadline);
                        let elapsed = started.elapsed();
                        let ticket = StartupTicket {
                            serial,
                            key: &key,
                            route,
                            initialize,
                        };
                        return self.finish_startup(ticket, outcome, elapsed, waited);
                    }
                    Step::Wait => waited += self.wait_slice(deadline, cancel)?,
                    Step::Failed(message) => return Err(io::Error::other(message)),
                }
            }
        })();
        self.leave_waiting(&mut waiting);
        outcome
    }

    /// Stop counting this caller as a waiter of a shared startup, in flight or
    /// already failed, so the failure record is dropped with its last waiter.
    fn leave_waiting(&self, waiting: &mut Option<u64>) {
        let Some(serial) = waiting.take() else {
            return;
        };
        let Ok(mut table) = self.table.lock() else {
            return;
        };
        if let Some(failed) = table.failed.get_mut(&serial) {
            failed.waiters = failed.waiters.saturating_sub(1);
            if failed.waiters == 0 {
                table.failed.remove(&serial);
            }
        } else if let Some(startup) = table
            .starting
            .values_mut()
            .find(|startup| startup.serial == serial)
        {
            startup.waiters = startup.waiters.saturating_sub(1);
        }
        self.changed.notify_all();
    }

    /// Resolve a completed shared startup: publish the ready worker and
    /// reserve it for this caller, or publish the failure for its waiters.
    fn finish_startup(
        &self,
        ticket: StartupTicket<'_>,
        outcome: io::Result<Box<dyn SharedWorker>>,
        started: Duration,
        queue_wait: Duration,
    ) -> io::Result<Lease> {
        let StartupTicket {
            serial,
            key: start_key,
            route,
            initialize,
        } = ticket;
        match outcome {
            Ok(worker) => {
                let identity = worker.identity();
                let used_ms = self.now_ms();
                // Native startup may register the project or create the
                // configuration files this identity hashes, so the identity
                // is recomputed after the worker exists; otherwise every
                // later request would miss the stale entry and replace a
                // healthy worker.
                let key = match serena_route::configuration_key(route, initialize, &self.home) {
                    Ok(key) => key,
                    Err(error) => {
                        let mut table = self.lock_table()?;
                        table.start.record(started);
                        let waiters = table.take_startup(start_key, serial);
                        table.startup_failures += 1;
                        if waiters > 0 {
                            table.failed.insert(
                                serial,
                                FailedStartup {
                                    message: error.to_string(),
                                    waiters,
                                },
                            );
                        }
                        self.changed.notify_all();
                        drop(table);
                        let mut worker = worker;
                        let _ = worker.close();
                        return Err(error);
                    }
                };
                let mut table = self.lock_table()?;
                table.start.record(started);
                if table.closed {
                    // The pool shut down while this worker started; it is
                    // closed here so no owned tree survives without an owner.
                    table.retiring += 1;
                    let _ = table.take_startup(start_key, serial);
                    self.changed.notify_all();
                    drop(table);
                    self.finish_close(worker);
                    return Err(io::Error::other("Serena pool is shutting down"));
                }
                let _ = table.take_startup(start_key, serial);
                // A compatible generation may have been published while this
                // worker started, for the same or the recomputed identity.
                if let Some(existing) = table.workers.get(&key).cloned()
                    && !existing.dead.load(Ordering::Acquire)
                {
                    existing.reservations.fetch_add(1, Ordering::AcqRel);
                    existing.used_ms.store(used_ms, Ordering::Release);
                    table.hits += 1;
                    self.changed.notify_all();
                    drop(table);
                    let mut worker = worker;
                    let _ = worker.close();
                    return Ok(Lease { slot: existing });
                }
                if let Some(stale) = table.workers.remove(&key) {
                    stale.reservations.store(0, Ordering::Release);
                    stale.dead.store(true, Ordering::Release);
                    table.retiring += 1;
                    self.changed.notify_all();
                    drop(table);
                    self.close_ready(&stale);
                    let mut table = self.lock_table()?;
                    let slot = self.publish(
                        &mut table,
                        Publication {
                            serial,
                            key: &key,
                            route,
                            identity,
                            worker,
                            used_ms,
                            queue_wait,
                        },
                    );
                    return Ok(Lease { slot });
                }
                let slot = self.publish(
                    &mut table,
                    Publication {
                        serial,
                        key: &key,
                        route,
                        identity,
                        worker,
                        used_ms,
                        queue_wait,
                    },
                );
                Ok(Lease { slot })
            }
            Err(error) => {
                let mut table = self.lock_table()?;
                table.start.record(started);
                let waiters = table.take_startup(start_key, serial);
                table.startup_failures += 1;
                if waiters > 0 {
                    table.failed.insert(
                        serial,
                        FailedStartup {
                            message: error.to_string(),
                            waiters,
                        },
                    );
                }
                self.changed.notify_all();
                Err(error)
            }
        }
    }

    /// Publish one freshly started generation under its recomputed identity.
    fn publish(&self, table: &mut Table, publication: Publication<'_>) -> Arc<Slot> {
        let Publication {
            serial,
            key,
            route,
            identity,
            worker,
            used_ms,
            queue_wait,
        } = publication;
        let slot = Arc::new(Slot {
            generation: serial,
            key: key.to_owned(),
            route: route.clone(),
            identity,
            gate: Mutex::new(Some(worker)),
            dead: AtomicBool::new(false),
            used_ms: AtomicU64::new(used_ms),
            reservations: AtomicUsize::new(1),
        });
        table.workers.insert(key.to_owned(), Arc::clone(&slot));
        table.cold_starts += 1;
        table.queue.record(queue_wait);
        self.changed.notify_all();
        slot
    }

    /// Remove a generation from the table, count it as retiring and close it
    /// when its gate is free; otherwise the current gate holder takes over the
    /// close after its bounded request.
    fn retire(&self, slot: &Arc<Slot>) {
        {
            let Ok(mut table) = self.table.lock() else {
                return;
            };
            if let Some(entry) = table.workers.get(&slot.key)
                && Arc::ptr_eq(entry, slot)
            {
                table.workers.remove(&slot.key);
                slot.reservations.store(0, Ordering::Release);
                table.retiring += 1;
            }
            slot.dead.store(true, Ordering::Release);
            self.changed.notify_all();
        }
        self.close_ready(slot);
    }

    /// Close a generation that already left the table and was counted as
    /// retiring.
    fn close_ready(&self, slot: &Arc<Slot>) {
        let taken = match slot.gate.try_lock() {
            Ok(mut gate) => gate.take(),
            Err(_) => None,
        };
        if let Some(worker) = taken {
            self.finish_close(worker);
        }
    }

    fn finish_close(&self, mut worker: Box<dyn SharedWorker>) {
        let _ = worker.close();
        if let Ok(mut table) = self.table.lock() {
            table.retiring = table.retiring.saturating_sub(1);
            self.changed.notify_all();
        }
    }

    /// Run one operation against a reserved worker. Infrastructure retries
    /// happen only before the operation ran: a retired, closed or dead worker
    /// is replaced, while a request error after execution is returned as is.
    fn with_worker<T, F>(
        &self,
        route: &Route,
        initialize: &Value,
        deadline: Deadline,
        cancel: &Cancellation,
        op: F,
    ) -> io::Result<(T, Arc<Slot>)>
    where
        F: Fn(&mut dyn SharedWorker) -> io::Result<T>,
    {
        loop {
            let lease = self.reserve(route, initialize, deadline, cancel)?;
            let slot = lease.slot;
            if cancel.is_cancelled() {
                self.release(&slot);
                return Err(interrupted("Serena request"));
            }
            if deadline.expired() {
                self.release(&slot);
                return Err(timed_out("Serena request before execution"));
            }
            let mut gate = match lock_until(&slot.gate, deadline, cancel, "Serena worker") {
                Ok(gate) => gate,
                Err(error) => {
                    self.release(&slot);
                    return Err(error);
                }
            };
            if slot.dead.load(Ordering::Acquire) {
                let taken = gate.take();
                drop(gate);
                if let Some(worker) = taken {
                    self.finish_close(worker);
                }
                self.release(&slot);
                continue;
            }
            let alive = gate.as_ref().is_some_and(|worker| worker.is_alive());
            if !alive {
                // The first detector counts the generation; whoever holds the
                // gate releases its process ownership.
                if !slot.dead.load(Ordering::Acquire) {
                    self.retire(&slot);
                }
                let taken = gate.take();
                drop(gate);
                if let Some(worker) = taken {
                    self.finish_close(worker);
                }
                self.release(&slot);
                continue;
            }
            let started = Instant::now();
            let outcome = match gate.as_mut() {
                Some(worker) => op(worker.as_mut()),
                None => Err(io::Error::other("Serena worker is closed")),
            };
            let elapsed = started.elapsed();
            if let Ok(mut table) = self.table.lock() {
                table.request.record(elapsed);
            }
            // A retirement that started while the request held the gate leaves
            // the close to the current holder.
            let taken = if slot.dead.load(Ordering::Acquire) {
                gate.take()
            } else {
                None
            };
            drop(gate);
            if let Some(worker) = taken {
                self.finish_close(worker);
            }
            self.release(&slot);
            return outcome.map(|value| (value, slot));
        }
    }

    /// Register a client from its original invocation. Returns the cached
    /// initialize result and the resolved route.
    pub fn connect(
        &self,
        client: &str,
        arguments: &[OsString],
        cwd: &Path,
        initialize: Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        if !valid_client(client) {
            return Err(invalid("Invalid Serena client identity"));
        }
        let route = serena_route::parse_route(arguments, cwd, &self.home)?;
        let entry = self.client_entry(client, &route, &initialize, true)?;
        let mut state = lock_until(&entry.state, deadline, cancel, "Serena client")?;
        state.used = Instant::now();
        let route = state.route.clone();
        let initialize = state.initialize.clone();
        let (message, _slot) =
            self.with_worker(&route, &initialize, deadline, cancel, |worker| {
                Ok(worker.initialized())
            })?;
        drop(state);
        Ok(json!({"message": message, "route": route.to_json()}))
    }

    /// Forward one client RPC. Unknown clients re-register from the supplied
    /// cached route, matching a proxy that outlived broker state.
    #[allow(clippy::too_many_arguments)] // One broker payload field per parameter.
    pub fn rpc(
        &self,
        client: &str,
        method: &str,
        params: Value,
        route: Route,
        initialize: Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        if !valid_client(client) {
            return Err(invalid("Invalid Serena client identity"));
        }
        let entry = self.client_entry(client, &route, &initialize, false)?;
        let mut state = lock_until(&entry.state, deadline, cancel, "Serena client")?;
        state.used = Instant::now();
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
            let resolved = serena_route::canonical_project(
                &selection,
                &route.cwd,
                &self.home,
                &state.known_projects,
                &state.removed_projects,
            )?;
            route.project = Some(resolved.clone());
            params["arguments"]["project"] = json!(resolved);
        }
        if removal {
            route.mutation_owner = Some(client.to_owned());
        }
        if method == "tools/call" {
            let mut meta = params.get("_meta").cloned().unwrap_or_else(|| json!({}));
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
        let mut attempt = 0u32;
        let result = loop {
            let (response, slot) =
                self.with_worker(&route, &initialize, deadline, cancel, |worker| {
                    worker.request(method, params.clone(), deadline)
                })?;
            if fatal_language_server(&response) {
                // A worker whose language-server manager failed stays alive but
                // can never answer semantic calls. Replace it and retry this
                // request once; nothing else is retried.
                self.retire(&slot);
                if attempt == 0 {
                    attempt = 1;
                    continue;
                }
            }
            break response;
        };
        let succeeded = result.get("error").is_none()
            && !result["result"]
                .get("isError")
                .is_some_and(|value| value != false);
        if activation && succeeded {
            if let Some(project) = &route.project {
                let value = project.to_string_lossy().into_owned();
                if !state.known_projects.contains(&value) {
                    state.known_projects.push(value);
                }
            }
            state.route = route.clone();
        }
        if removal && succeeded {
            if let Some(name) = removed_name
                && !state.route.removed_projects.contains(&name)
            {
                state.route.removed_projects.push(name);
            }
            route.removed_projects = state.route.removed_projects.clone();
            state.route = route;
        }
        Ok(json!({
            "message": result,
            "route": state.route.to_json(),
            "tools_changed": activation && succeeded,
        }))
    }

    pub fn disconnect(&self, client: &str) -> bool {
        self.lock_table()
            .map(|mut table| {
                let removed = table.clients.remove(client).is_some();
                if removed {
                    self.changed.notify_all();
                }
                removed
            })
            .unwrap_or(false)
    }

    /// Close idle or dead workers and forget idle clients. Liveness and close
    /// run outside the table lock; only idle generations are removed.
    pub fn reap(&self) {
        let idle = Duration::from_secs(self.policy.idle_seconds);
        let now = Instant::now();
        let idle_ms = millis(idle);
        let now_ms = self.now_ms();
        {
            let Ok(mut table) = self.table.lock() else {
                return;
            };
            if table.closed {
                return;
            }
            // A client mid-operation holds its state; it is never reaped here.
            table
                .clients
                .retain(|_, entry| match entry.state.try_lock() {
                    Ok(state) => now.saturating_duration_since(state.used) < idle,
                    Err(_) => true,
                });
        }
        let candidates: Vec<Arc<Slot>> = match self.table.lock() {
            Ok(table) => table
                .workers
                .values()
                .filter(|slot| slot.reservations.load(Ordering::Acquire) == 0)
                .cloned()
                .collect(),
            Err(_) => return,
        };
        for slot in candidates {
            let Ok(mut gate) = slot.gate.try_lock() else {
                continue;
            };
            let Some(alive) = gate.as_ref().map(|worker| worker.is_alive()) else {
                continue;
            };
            let remove = match self.table.lock() {
                Ok(mut table) => {
                    let expired = table.workers.get(&slot.key).is_some_and(|entry| {
                        now_ms.saturating_sub(entry.used_ms.load(Ordering::Acquire)) >= idle_ms
                    });
                    if !table.closed
                        && (!alive || expired)
                        && let Some(entry) = table.workers.get(&slot.key)
                        && Arc::ptr_eq(entry, &slot)
                        && entry.reservations.load(Ordering::Acquire) == 0
                    {
                        table.workers.remove(&slot.key);
                        slot.reservations.store(0, Ordering::Release);
                        table.retiring += 1;
                        true
                    } else {
                        false
                    }
                }
                Err(_) => false,
            };
            if remove {
                slot.dead.store(true, Ordering::Release);
                let worker = gate.take().expect("the gate was inspected while held");
                drop(gate);
                self.changed.notify_all();
                self.finish_close(worker);
            }
        }
    }

    /// Retire every owned generation within the supplied deadline. In-flight
    /// requests close their own generation after they finish; an unconfirmed
    /// close reports the deadline instead of pretending the pool is empty.
    pub fn close(&self, deadline: Deadline) -> io::Result<()> {
        {
            let mut table = self.lock_table()?;
            table.closed = true;
            table.clients.clear();
            self.changed.notify_all();
        }
        loop {
            let batch: Vec<Arc<Slot>> = {
                let mut table = self.lock_table()?;
                if table.workers.is_empty() && table.starting.is_empty() && table.retiring == 0 {
                    return Ok(());
                }
                let drained: Vec<Arc<Slot>> =
                    std::mem::take(&mut table.workers).into_values().collect();
                for slot in &drained {
                    slot.reservations.store(0, Ordering::Release);
                    slot.dead.store(true, Ordering::Release);
                }
                table.retiring += drained.len();
                if !drained.is_empty() {
                    self.changed.notify_all();
                }
                drained
            };
            for slot in batch {
                self.close_ready(&slot);
            }
            if deadline.expired() {
                return Err(timed_out("Serena pool close before ownership was released"));
            }
            let slice = deadline.remaining().min(WAIT_SLICE);
            let table = self.lock_table()?;
            let _ = self
                .changed
                .wait_timeout(table, slice)
                .map_err(|_| io::Error::other("Serena pool table lock poisoned"))?;
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        if let Ok(deadline) = Deadline::after(Duration::from_secs(5)) {
            let _ = self.close(deadline);
        }
    }
}

/// The real worker factory around the adopted Serena entry point. The cancel
/// token owns every worker's pipe lifetime and is never cancelled; individual
/// requests carry their own deadlines.
pub fn session_factory(
    launch: serena::Launch,
    cancel: crate::process::Cancellation,
) -> io::Result<WorkerFactory> {
    let stderr_root = launch.home.join("harness/runtime/serena-workers");
    std::fs::create_dir_all(&stderr_root)?;
    let serial = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    Ok(Box::new(move |route, initialize, deadline| {
        let command = serena::shared_command(&launch, route)?;
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
            // Job wait already stopped members and confirmed an empty tree.
            // Timeout/memory reasons still completed cleanup; failing connect
            // here bricks every new Codex session that reuses the broker.
            let _outcome = session.close()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        fs,
        sync::atomic::AtomicU64,
        time::{Duration, Instant},
    };

    const WAIT: Duration = Duration::from_secs(10);

    /// A reusable boolean signal for holding fake work until the test releases
    /// it.
    #[derive(Default)]
    struct Signal {
        state: Mutex<bool>,
        changed: Condvar,
    }

    impl Signal {
        fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        fn set(&self) {
            *self.state.lock().unwrap() = true;
            self.changed.notify_all();
        }

        fn is_set(&self) -> bool {
            *self.state.lock().unwrap()
        }

        fn wait(&self, timeout: Duration) -> bool {
            let deadline = Instant::now() + timeout;
            let mut state = self.state.lock().unwrap();
            while !*state {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return false;
                }
                let (guard, _) = self.changed.wait_timeout(state, left).unwrap();
                state = guard;
            }
            true
        }
    }

    /// A counting barrier: waits until `target` callers arrived.
    struct Barrier {
        target: usize,
        arrived: Mutex<usize>,
        changed: Condvar,
    }

    impl Barrier {
        fn new(target: usize) -> Arc<Self> {
            Arc::new(Self {
                target,
                arrived: Mutex::new(0),
                changed: Condvar::new(),
            })
        }

        fn arrive(&self, timeout: Duration) -> bool {
            let deadline = Instant::now() + timeout;
            let mut arrived = self.arrived.lock().unwrap();
            *arrived += 1;
            if *arrived >= self.target {
                self.changed.notify_all();
                return true;
            }
            while *arrived < self.target {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return false;
                }
                let (guard, _) = self.changed.wait_timeout(arrived, left).unwrap();
                arrived = guard;
            }
            true
        }
    }

    fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if predicate() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        predicate()
    }

    /// Per-project fake behavior; every synchronization is explicit so a
    /// concurrency defect fails the test instead of hanging it.
    #[derive(Clone, Default)]
    struct Plan {
        start_hold: Option<Arc<Signal>>,
        start_failure: bool,
        request_hold: Option<Arc<Signal>>,
        /// Restrict `request_hold` to the fake worker with this index, so a
        /// replacement generation can serve while the first one is held.
        request_hold_index: Option<u64>,
        /// File written by the fake factory before it returns a worker,
        /// standing in for the project registration a native startup does.
        settle: Option<(PathBuf, String)>,
        request_barrier: Option<Arc<Barrier>>,
        request_failure: bool,
        close_hold: Option<Arc<Signal>>,
        fatal_calls: Option<Arc<AtomicU64>>,
    }

    #[derive(Default)]
    struct FakeState {
        alive: bool,
        closed: bool,
        closes: usize,
        requests: Vec<(String, Value)>,
        active: usize,
        peak_active: usize,
    }

    struct FakeWorker {
        index: u64,
        state: Arc<Mutex<FakeState>>,
        plan: Plan,
    }

    impl FakeWorker {
        fn fixture_initialize(&self) -> Value {
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {"serverInfo": {"name": "Serena"}, "fixture": self.index},
            })
        }

        fn serve(&self, method: &str, params: &Value) -> io::Result<Value> {
            if let Some(barrier) = &self.plan.request_barrier
                && !barrier.arrive(WAIT)
            {
                return Err(io::Error::other("fixture request barrier timed out"));
            }
            if let Some(hold) = &self.plan.request_hold
                && self
                    .plan
                    .request_hold_index
                    .is_none_or(|index| index == self.index)
                && !hold.wait(WAIT)
            {
                return Err(io::Error::other("fixture request hold timed out"));
            }
            if self.plan.request_failure {
                return Err(io::Error::other("fixture request outcome unknown"));
            }
            if !self.state.lock().unwrap().alive {
                return Err(io::Error::other("fixture worker is unavailable"));
            }
            if method == "tools/call"
                && let Some(fatal) = &self.plan.fatal_calls
                && fatal.load(Ordering::SeqCst) > 0
            {
                fatal.fetch_sub(1, Ordering::SeqCst);
                return Ok(fatal_ls_message());
            }
            Ok(json!({
                "jsonrpc": "2.0",
                "id": 7,
                "result": {"content": [], "structuredContent": {"fixture": self.index, "params": params}},
            }))
        }
    }

    impl SharedWorker for FakeWorker {
        fn request(
            &mut self,
            method: &str,
            params: Value,
            _deadline: Deadline,
        ) -> io::Result<Value> {
            {
                let mut state = self.state.lock().unwrap();
                state.requests.push((method.to_owned(), params.clone()));
                state.active += 1;
                state.peak_active = state.peak_active.max(state.active);
            }
            let outcome = self.serve(method, &params);
            self.state.lock().unwrap().active -= 1;
            outcome
        }

        fn initialized(&self) -> Value {
            self.fixture_initialize()
        }

        fn identity(&self) -> ProcessIdentity {
            ProcessIdentity {
                pid: 1000 + self.index as u32,
                creation_time: self.index,
            }
        }

        fn is_alive(&self) -> bool {
            self.state.lock().unwrap().alive
        }

        fn close(&mut self) -> io::Result<()> {
            if let Some(hold) = &self.plan.close_hold {
                let _ = hold.wait(WAIT);
            }
            let mut state = self.state.lock().unwrap();
            state.alive = false;
            state.closed = true;
            state.closes += 1;
            Ok(())
        }
    }

    struct Fixture {
        _root: tempfile::TempDir,
        home: PathBuf,
        starts: Arc<AtomicU64>,
        plans: Arc<Mutex<BTreeMap<PathBuf, Plan>>>,
        states: Arc<Mutex<Vec<Arc<Mutex<FakeState>>>>>,
    }

    impl Fixture {
        fn new(tag: &str, max_projects: usize, idle_seconds: u64) -> (Self, Pool) {
            Self::with_startup_bound(tag, max_projects, idle_seconds, 4)
        }

        fn with_startup_bound(
            tag: &str,
            max_projects: usize,
            idle_seconds: u64,
            max_concurrent_startups: usize,
        ) -> (Self, Pool) {
            let root = tempfile::Builder::new()
                .prefix(&format!("harness-serena-pool-{tag}-"))
                .tempdir()
                .unwrap();
            let home = root.path().join("serena-home");
            fs::create_dir_all(home.join("contexts")).unwrap();
            fs::write(home.join("serena_config.yml"), "projects: []\n").unwrap();
            fs::write(home.join("contexts/codex.yml"), "tools: []\n").unwrap();
            let starts = Arc::new(AtomicU64::new(0));
            let plans: Arc<Mutex<BTreeMap<PathBuf, Plan>>> = Arc::new(Mutex::new(BTreeMap::new()));
            let states: Arc<Mutex<Vec<Arc<Mutex<FakeState>>>>> = Arc::new(Mutex::new(Vec::new()));
            let factory_starts = Arc::clone(&starts);
            let factory_plans = Arc::clone(&plans);
            let factory_states = Arc::clone(&states);
            let factory: WorkerFactory = Box::new(move |route, _initialize, _deadline| {
                let project = route
                    .project
                    .as_ref()
                    .and_then(|project| fs::canonicalize(project).ok());
                let plan = project
                    .and_then(|project| factory_plans.lock().unwrap().get(&project).cloned())
                    .unwrap_or_default();
                let index = factory_starts.fetch_add(1, Ordering::SeqCst);
                if let Some(hold) = &plan.start_hold
                    && !hold.wait(WAIT)
                {
                    return Err(io::Error::other("fixture startup hold timed out"));
                }
                if plan.start_failure {
                    return Err(io::Error::other("fixture startup failure"));
                }
                if let Some((path, contents)) = &plan.settle {
                    fs::write(path, contents).unwrap();
                }
                let state = Arc::new(Mutex::new(FakeState {
                    alive: true,
                    ..FakeState::default()
                }));
                factory_states.lock().unwrap().push(Arc::clone(&state));
                Ok(Box::new(FakeWorker { index, state, plan }) as Box<dyn SharedWorker>)
            });
            let pool = Pool::new(
                serena_route::Policy {
                    max_projects,
                    idle_seconds,
                    max_concurrent_startups,
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
                    plans,
                    states,
                },
                pool,
            )
        }

        fn project(&self, name: &str) -> PathBuf {
            let project = self.home.parent().unwrap().join(name);
            fs::create_dir_all(project.join(".serena")).unwrap();
            fs::write(
                project.join(".serena/project.yml"),
                format!("project_name: '{name}'\n"),
            )
            .unwrap();
            project
        }

        fn plan(&self, project: &Path, plan: Plan) {
            self.plans
                .lock()
                .unwrap()
                .insert(fs::canonicalize(project).unwrap(), plan);
        }

        fn states(&self) -> Vec<Arc<Mutex<FakeState>>> {
            self.states.lock().unwrap().clone()
        }

        fn state(&self, index: usize) -> Arc<Mutex<FakeState>> {
            self.states()[index].clone()
        }

        fn closed(&self, index: usize) -> bool {
            self.state(index).lock().unwrap().closed
        }

        fn closed_count(&self) -> usize {
            self.states()
                .iter()
                .filter(|state| state.lock().unwrap().closed)
                .count()
        }

        fn requests(&self, index: usize) -> Vec<(String, Value)> {
            self.state(index).lock().unwrap().requests.clone()
        }

        fn starts(&self) -> u64 {
            self.starts.load(Ordering::SeqCst)
        }

        fn wait_requests(&self, index: usize, count: usize) -> bool {
            wait_until(WAIT, || self.requests(index).len() >= count)
        }

        fn client(id: usize) -> String {
            format!("{:032x}", id)
        }

        fn deadline() -> Deadline {
            Deadline::after(Duration::from_secs(30)).unwrap()
        }
    }

    fn initialize() -> Value {
        json!({"protocolVersion": "2024-11-05"})
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

    fn fatal_ls_message() -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "result": {
                "isError": true,
                "content": [{
                    "type": "text",
                    "text": "Error executing tool find_symbol: Exception: The language server manager is not initialized, indicating a problem during project initialisation."
                }]
            }
        })
    }

    fn managed_route(project: &Path) -> Route {
        Route {
            project: Some(project.to_path_buf()),
            cwd: project.to_path_buf(),
            arguments: vec![
                "start-mcp-server".into(),
                "--context".into(),
                "codex".into(),
            ],
            removed_projects: Vec::new(),
            mutation_owner: None,
        }
    }

    fn connect(pool: &Pool, client: &str, project: &Path) -> io::Result<Value> {
        pool.connect(
            client,
            &managed_arguments(project),
            project,
            initialize(),
            Fixture::deadline(),
            &Cancellation::default(),
        )
    }

    fn rpc(
        pool: &Pool,
        client: &str,
        project: &Path,
        method: &str,
        params: Value,
    ) -> io::Result<Value> {
        pool.rpc(
            client,
            method,
            params,
            managed_route(project),
            initialize(),
            Fixture::deadline(),
            &Cancellation::default(),
        )
    }

    fn rpc_with(
        pool: &Pool,
        client: &str,
        project: &Path,
        method: &str,
        params: Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        pool.rpc(
            client,
            method,
            params,
            managed_route(project),
            initialize(),
            deadline,
            cancel,
        )
    }

    fn spawn_rpc(
        pool: &Arc<Pool>,
        client: &str,
        project: &Path,
        method: &'static str,
        params: Value,
    ) -> std::thread::JoinHandle<io::Result<Value>> {
        let pool = Arc::clone(pool);
        let client = client.to_owned();
        let project = project.to_path_buf();
        std::thread::spawn(move || rpc(&pool, &client, &project, method, params))
    }

    fn content(response: &Value) -> &Value {
        &response["message"]["result"]["structuredContent"]
    }

    #[test]
    fn only_the_fatal_language_server_marker_retires_a_worker() {
        assert!(fatal_language_server(&fatal_ls_message()));
        let ordinary = json!({"result": {"isError": true, "content": [
            {"type": "text", "text": "Error executing tool find_symbol: symbol not found"}
        ]}});
        assert!(!fatal_language_server(&ordinary));
        assert!(!fatal_language_server(&json!({"result": {}})));
    }

    #[test]
    fn same_selection_clients_share_one_worker() {
        let (fixture, pool) = Fixture::new("share", 3, 300);
        let project = fixture.project("alpha");
        let first = connect(&pool, &Fixture::client(1), &project).unwrap();
        assert_eq!(first["message"]["result"]["serverInfo"]["name"], "Serena");
        let second = connect(&pool, &Fixture::client(2), &project).unwrap();
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
        assert_eq!(status["counters"]["cold_starts"], 1);
        assert_eq!(status["counters"]["hits"], 1);
        pool.close(Fixture::deadline()).unwrap();
        assert!(fixture.closed(0));
    }

    #[test]
    fn capacity_evicts_the_least_recently_used_worker() {
        let (fixture, pool) = Fixture::new("capacity", 2, 300);
        let first = fixture.project("one");
        let second = fixture.project("two");
        let third = fixture.project("three");
        for (index, project) in [&first, &second, &third].into_iter().enumerate() {
            connect(&pool, &Fixture::client(index + 1), project).unwrap();
        }
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 3);
        assert_eq!(pool.worker_count(), 2);
        assert_eq!(fixture.closed_count(), 1);
        assert_eq!(pool.status()["counters"]["evictions"], 1);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn dead_worker_is_replaced_on_the_next_request() {
        let (fixture, pool) = Fixture::new("dead", 3, 300);
        let project = fixture.project("dead-project");
        let client = Fixture::client(7);
        connect(&pool, &client, &project).unwrap();
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        fixture.state(0).lock().unwrap().alive = false;
        let result = rpc(&pool, &client, &project, "tools/list", json!({})).unwrap();
        assert!(!content(&result).is_null());
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 2);
        assert!(fixture.closed(0));
        assert!(
            fixture.requests(0).is_empty(),
            "a dead worker never receives the request"
        );
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn fatal_language_server_is_retried_once_on_a_fresh_worker() {
        let (fixture, pool) = Fixture::new("fatal-retry", 3, 300);
        let project = fixture.project("retry-project");
        let client = Fixture::client(11);
        fixture.plan(
            &project,
            Plan {
                fatal_calls: Some(Arc::new(AtomicU64::new(1))),
                ..Plan::default()
            },
        );
        connect(&pool, &client, &project).unwrap();
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        let result = rpc(
            &pool,
            &client,
            &project,
            "tools/call",
            json!({"name": "find_symbol", "arguments": {"name_path_pattern": "shared"}}),
        )
        .unwrap();
        assert_eq!(content(&result)["fixture"], 1);
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 2);
        assert!(fixture.closed(0));
        assert!(!fixture.closed(1));
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn persistent_fatal_language_server_is_returned_after_one_retry() {
        let (fixture, pool) = Fixture::new("fatal-persist", 3, 300);
        let project = fixture.project("persist-project");
        let client = Fixture::client(12);
        fixture.plan(
            &project,
            Plan {
                fatal_calls: Some(Arc::new(AtomicU64::new(2))),
                ..Plan::default()
            },
        );
        connect(&pool, &client, &project).unwrap();
        let result = rpc(
            &pool,
            &client,
            &project,
            "tools/call",
            json!({"name": "find_symbol", "arguments": {}}),
        )
        .unwrap();
        let text = result["message"]["result"]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert!(
            text.contains("The language server manager is not initialized"),
            "{text}"
        );
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 2);
        assert!(fixture.closed(0) && fixture.closed(1));
        assert_eq!(fixture.requests(0).len(), 1);
        assert_eq!(fixture.requests(1).len(), 1);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn request_errors_are_not_retried_on_a_fresh_worker() {
        let (fixture, pool) = Fixture::new("no-retry", 3, 300);
        let project = fixture.project("no-retry-project");
        fixture.plan(
            &project,
            Plan {
                request_failure: true,
                ..Plan::default()
            },
        );
        connect(&pool, &Fixture::client(1), &project).unwrap();
        let error = rpc(
            &pool,
            &Fixture::client(1),
            &project,
            "tools/call",
            json!({"name": "rename_symbol", "arguments": {}}),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            fixture.requests(0).len(),
            1,
            "an uncertain mutation is not retried"
        );
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        assert!(!fixture.closed(0));
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn configuration_change_replaces_the_worker_for_that_selection() {
        let (fixture, pool) = Fixture::new("config", 3, 300);
        let project = fixture.project("configured");
        let client = Fixture::client(9);
        connect(&pool, &client, &project).unwrap();
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        fs::write(
            fixture.home.join("contexts/codex.yml"),
            "tools:\n  - find_symbol\n",
        )
        .unwrap();
        rpc(&pool, &client, &project, "tools/list", json!({})).unwrap();
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 2);
        assert!(fixture.closed(0));
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn configuration_replacement_waits_for_in_flight_work() {
        let (fixture, pool) = Fixture::new("replace", 3, 300);
        let project = fixture.project("replace-project");
        let hold = Signal::new();
        fixture.plan(
            &project,
            Plan {
                request_hold: Some(Arc::clone(&hold)),
                request_hold_index: Some(0),
                ..Plan::default()
            },
        );
        connect(&pool, &Fixture::client(1), &project).unwrap();
        let pool = Arc::new(pool);
        let in_flight = spawn_rpc(
            &pool,
            &Fixture::client(1),
            &project,
            "tools/list",
            json!({"old": true}),
        );
        assert!(fixture.wait_requests(0, 1));
        fs::write(
            fixture.home.join("contexts/codex.yml"),
            "tools:\n  - find_symbol\n",
        )
        .unwrap();
        let replaced = rpc(
            &pool,
            &Fixture::client(2),
            &project,
            "tools/list",
            json!({}),
        )
        .unwrap();
        assert_eq!(content(&replaced)["fixture"], 1);
        assert_eq!(pool.status()["counters"]["cold_starts"], 2);
        assert!(
            !fixture.closed(0),
            "an in-flight generation is never evicted by a replacement"
        );
        hold.set();
        let in_flight = in_flight.join().unwrap().unwrap();
        assert_eq!(content(&in_flight)["fixture"], 0);
        // The replacement keeps serving its own configuration and never
        // reuses the obsolete generation.
        let current = rpc(
            &pool,
            &Fixture::client(2),
            &project,
            "tools/list",
            json!({}),
        )
        .unwrap();
        assert_eq!(content(&current)["fixture"], 1);
        assert_eq!(fixture.requests(0).len(), 1);
        assert_eq!(pool.status()["counters"]["cold_starts"], 2);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn different_projects_cold_start_serially_under_the_startup_bound() {
        // A burst of concurrent sessions must not stack fresh language-server
        // trees on one host: with one concurrent cold start, a second
        // project's startup waits for the first to finish instead of running
        // beside it. The bound addresses machine-wide startup exhaustion; it
        // never weakens a language, a worker limit or the single retry.
        let (fixture, pool) = Fixture::with_startup_bound("serial-start", 3, 300, 1);
        let alpha = fixture.project("alpha-serial");
        let beta = fixture.project("beta-serial");
        let hold = Signal::new();
        fixture.plan(
            &alpha,
            Plan {
                start_hold: Some(Arc::clone(&hold)),
                ..Plan::default()
            },
        );
        let pool = Arc::new(pool);
        let first = spawn_rpc(&pool, &Fixture::client(1), &alpha, "tools/list", json!({}));
        assert!(
            wait_until(WAIT, || fixture.starts() == 1),
            "the first startup is in flight"
        );
        let second = spawn_rpc(&pool, &Fixture::client(2), &beta, "tools/list", json!({}));
        assert!(
            wait_until(WAIT, || {
                pool.status()["counters"]["serialized_startups"]
                    .as_u64()
                    .unwrap_or(0)
                    >= 1
            }),
            "the second startup reached the bound"
        );
        assert_eq!(
            fixture.starts(),
            1,
            "the second factory cannot start while the first is in flight"
        );
        hold.set();
        let first = first.join().unwrap().unwrap();
        let second = second.join().unwrap().unwrap();
        assert_eq!(content(&first)["fixture"], 0);
        assert_eq!(content(&second)["fixture"], 1);
        assert_eq!(fixture.starts(), 2);
        assert_eq!(pool.status()["counters"]["cold_starts"], 2);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn activate_project_updates_the_route_and_reports_tools_changed() {
        let (fixture, pool) = Fixture::new("activate", 3, 300);
        let alpha = fixture.project("alpha-route");
        let beta = fixture.project("beta-route");
        let client = Fixture::client(11);
        let connected = connect(&pool, &client, &alpha).unwrap();
        assert_eq!(connected["route"]["project"], json!(alpha));
        let result = rpc(
            &pool,
            &client,
            &alpha,
            "tools/call",
            json!({"name": "activate_project", "arguments": {"project": beta.to_string_lossy()}}),
        )
        .unwrap();
        assert_eq!(result["tools_changed"], true);
        let canonical = crate::dependency_package::resolved(&beta).unwrap();
        assert_eq!(result["route"]["project"], json!(canonical));
        let requests = fixture.requests(1);
        let activation = requests
            .iter()
            .find(|(method, _)| method == "tools/call")
            .unwrap();
        assert_eq!(
            activation.1["_meta"]["harness_serena_client"],
            json!(client)
        );
        assert_eq!(activation.1["arguments"]["project"], json!(canonical));
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn registered_names_resolve_to_one_canonical_root() {
        let (fixture, pool) = Fixture::new("names", 3, 300);
        let alpha = fixture.project("alpha");
        let beta = fixture.project("beta");
        fs::write(
            fixture.home.join("serena_config.yml"),
            format!(
                "projects:\n  - {}\n  - {}\n",
                alpha.to_string_lossy(),
                beta.to_string_lossy()
            ),
        )
        .unwrap();
        connect(&pool, &Fixture::client(1), &alpha).unwrap();
        connect(&pool, &Fixture::client(2), &alpha).unwrap();
        // One client's ordered activation resolves the registered name and
        // moves only its own selection; the other client keeps its results.
        let activated = rpc(
            &pool,
            &Fixture::client(2),
            &alpha,
            "tools/call",
            json!({"name": "activate_project", "arguments": {"project": "beta"}}),
        )
        .unwrap();
        assert_eq!(activated["tools_changed"], true);
        let canonical = crate::dependency_package::resolved(&beta).unwrap();
        assert_eq!(activated["route"]["project"], json!(canonical));
        let second = rpc(&pool, &Fixture::client(2), &alpha, "tools/list", json!({})).unwrap();
        assert_eq!(content(&second)["fixture"], 1);
        assert_eq!(second["route"]["project"], json!(canonical));
        let first = rpc(&pool, &Fixture::client(1), &alpha, "tools/list", json!({})).unwrap();
        assert_eq!(content(&first)["fixture"], 0);
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 2);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn remove_project_marks_the_client_incompatible_before_mutation() {
        let (fixture, pool) = Fixture::new("remove", 3, 300);
        let project = fixture.project("removal");
        let client = Fixture::client(13);
        connect(&pool, &client, &project).unwrap();
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        let result = rpc(
            &pool,
            &client,
            &project,
            "tools/call",
            json!({"name": "remove_project", "arguments": {"project_name": "legacy"}}),
        )
        .unwrap();
        assert_eq!(result["route"]["removed_projects"], json!(["legacy"]));
        assert_eq!(result["route"]["mutation_owner"], json!(client));
        // The mutating client needs a fresh worker for its changed identity.
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 2);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn client_identity_and_capacity_rules_are_enforced() {
        let (fixture, pool) = Fixture::new("clients", 3, 300);
        let project = fixture.project("clients-project");
        assert!(connect(&pool, "short", &project).is_err());
        for index in 0..128 {
            connect(&pool, &Fixture::client(index + 1), &project).unwrap();
        }
        assert!(connect(&pool, &Fixture::client(200), &project).is_err());
        assert_eq!(pool.client_count(), 128);
        assert!(pool.disconnect(&Fixture::client(1)));
        assert!(!pool.disconnect(&Fixture::client(1)));
        connect(&pool, &Fixture::client(200), &project).unwrap();
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn unknown_rpc_client_registers_from_its_cached_route() {
        let (fixture, pool) = Fixture::new("unknown", 3, 300);
        let project = fixture.project("unknown-project");
        let client = Fixture::client(21);
        let result = rpc(&pool, &client, &project, "tools/list", json!({})).unwrap();
        assert!(!content(&result).is_null());
        assert_eq!(pool.client_count(), 1);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn reap_closes_idle_workers_and_forgets_idle_clients() {
        let (fixture, pool) = Fixture::new("reap", 3, 1);
        let project = fixture.project("idle");
        let client = Fixture::client(31);
        connect(&pool, &client, &project).unwrap();
        assert_eq!(pool.worker_count(), 1);
        std::thread::sleep(Duration::from_millis(1100));
        pool.reap();
        assert_eq!(pool.worker_count(), 0);
        assert_eq!(pool.client_count(), 0);
        assert!(fixture.closed(0));
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn independent_workers_overlap_before_either_is_released() {
        let (fixture, pool) = Fixture::new("overlap", 3, 300);
        let alpha = fixture.project("overlap-alpha");
        let beta = fixture.project("overlap-beta");
        let barrier = Barrier::new(2);
        fixture.plan(
            &alpha,
            Plan {
                request_barrier: Some(Arc::clone(&barrier)),
                ..Plan::default()
            },
        );
        fixture.plan(
            &beta,
            Plan {
                request_barrier: Some(Arc::clone(&barrier)),
                ..Plan::default()
            },
        );
        connect(&pool, &Fixture::client(1), &alpha).unwrap();
        connect(&pool, &Fixture::client(2), &beta).unwrap();
        let pool = Arc::new(pool);
        let first = spawn_rpc(&pool, &Fixture::client(1), &alpha, "tools/list", json!({}));
        let second = spawn_rpc(&pool, &Fixture::client(2), &beta, "tools/list", json!({}));
        let first = first.join().unwrap().unwrap();
        let second = second.join().unwrap().unwrap();
        assert_eq!(content(&first)["fixture"], 0);
        assert_eq!(content(&second)["fixture"], 1);
        assert_eq!(fixture.state(0).lock().unwrap().peak_active, 1);
        assert_eq!(fixture.state(1).lock().unwrap().peak_active, 1);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn one_worker_serializes_concurrent_clients() {
        let (fixture, pool) = Fixture::new("serialize", 3, 300);
        let project = fixture.project("serialize-project");
        let hold = Signal::new();
        fixture.plan(
            &project,
            Plan {
                request_hold: Some(Arc::clone(&hold)),
                ..Plan::default()
            },
        );
        connect(&pool, &Fixture::client(1), &project).unwrap();
        connect(&pool, &Fixture::client(2), &project).unwrap();
        let pool = Arc::new(pool);
        let first = spawn_rpc(
            &pool,
            &Fixture::client(1),
            &project,
            "tools/list",
            json!({"caller": "first"}),
        );
        assert!(fixture.wait_requests(0, 1));
        let second = spawn_rpc(
            &pool,
            &Fixture::client(2),
            &project,
            "tools/list",
            json!({"caller": "second"}),
        );
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            fixture.requests(0).len(),
            1,
            "a second client of one worker must wait for the in-flight request"
        );
        hold.set();
        let first = first.join().unwrap().unwrap();
        let second = second.join().unwrap().unwrap();
        assert_eq!(content(&first)["params"]["caller"], "first");
        assert_eq!(content(&second)["params"]["caller"], "second");
        assert_eq!(fixture.state(0).lock().unwrap().peak_active, 1);
        assert_eq!(fixture.requests(0).len(), 2);
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn startup_registration_settles_before_the_first_reuse() {
        let (fixture, pool) = Fixture::new("settle", 3, 300);
        let project = fixture.project("settle-project");
        // A native startup may register the project or create hashed
        // configuration; the settled identity must be reused by the next
        // request instead of replacing the healthy worker.
        fixture.plan(
            &project,
            Plan {
                settle: Some((
                    fixture.home.join("contexts/codex.yml"),
                    "tools:\n  - find_symbol\n".to_owned(),
                )),
                ..Plan::default()
            },
        );
        let client = Fixture::client(1);
        connect(&pool, &client, &project).unwrap();
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        rpc(&pool, &client, &project, "tools/list", json!({})).unwrap();
        assert_eq!(
            fixture.starts.load(Ordering::SeqCst),
            1,
            "a settling startup must not cause a replacement"
        );
        assert_eq!(fixture.closed_count(), 0);
        let status = pool.status();
        assert_eq!(status["counters"]["cold_starts"], 1);
        assert_eq!(status["counters"]["hits"], 1);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn compatible_cold_requests_share_one_startup() {
        let (fixture, pool) = Fixture::new("colds", 3, 300);
        let project = fixture.project("cold-project");
        let hold = Signal::new();
        fixture.plan(
            &project,
            Plan {
                start_hold: Some(Arc::clone(&hold)),
                ..Plan::default()
            },
        );
        let pool = Arc::new(pool);
        let start = Barrier::new(3);
        let mut threads = Vec::new();
        for id in 1..=3 {
            let pool = Arc::clone(&pool);
            let project = project.clone();
            let start = Arc::clone(&start);
            threads.push(std::thread::spawn(move || {
                assert!(start.arrive(WAIT));
                rpc(
                    &pool,
                    &Fixture::client(id),
                    &project,
                    "tools/list",
                    json!({}),
                )
            }));
        }
        assert!(wait_until(WAIT, || fixture.starts.load(Ordering::SeqCst) == 1));
        hold.set();
        for thread in threads {
            thread.join().unwrap().unwrap();
        }
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.states().len(), 1);
        let status = pool.status();
        assert_eq!(status["counters"]["cold_starts"], 1);
        assert_eq!(status["counters"]["hits"], 2);
        assert_eq!(pool.worker_count(), 1);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn warm_project_progresses_during_another_startup() {
        let (fixture, pool) = Fixture::new("warm", 3, 300);
        let warm = fixture.project("warm-project");
        let cold = fixture.project("cold-project");
        let hold = Signal::new();
        fixture.plan(
            &cold,
            Plan {
                start_hold: Some(Arc::clone(&hold)),
                ..Plan::default()
            },
        );
        connect(&pool, &Fixture::client(1), &warm).unwrap();
        let pool = Arc::new(pool);
        let pending = spawn_rpc(&pool, &Fixture::client(2), &cold, "tools/list", json!({}));
        assert!(wait_until(WAIT, || fixture.starts.load(Ordering::SeqCst) == 2));
        let response = rpc(&pool, &Fixture::client(1), &warm, "tools/list", json!({})).unwrap();
        assert_eq!(content(&response)["fixture"], 0);
        assert!(
            !hold.is_set(),
            "the unrelated startup is still pending while the warm project answers"
        );
        hold.set();
        pending.join().unwrap().unwrap();
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 2);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn responses_stay_bound_to_their_own_root_and_client() {
        let (fixture, pool) = Fixture::new("binding", 3, 300);
        let alpha = fixture.project("bind-alpha");
        let beta = fixture.project("bind-beta");
        connect(&pool, &Fixture::client(1), &alpha).unwrap();
        connect(&pool, &Fixture::client(2), &beta).unwrap();
        let pool = Arc::new(pool);
        let first = spawn_rpc(
            &pool,
            &Fixture::client(1),
            &alpha,
            "tools/call",
            json!({"name": "find_symbol", "arguments": {"root": "alpha"}}),
        );
        let second = spawn_rpc(
            &pool,
            &Fixture::client(2),
            &beta,
            "tools/call",
            json!({"name": "find_symbol", "arguments": {"root": "beta"}}),
        );
        let first = first.join().unwrap().unwrap();
        let second = second.join().unwrap().unwrap();
        assert_eq!(content(&first)["fixture"], 0);
        assert_eq!(content(&first)["params"]["arguments"]["root"], "alpha");
        assert_eq!(
            content(&first)["params"]["_meta"]["harness_serena_client"],
            json!(Fixture::client(1))
        );
        assert_eq!(content(&second)["fixture"], 1);
        assert_eq!(content(&second)["params"]["arguments"]["root"], "beta");
        assert_eq!(
            content(&second)["params"]["_meta"]["harness_serena_client"],
            json!(Fixture::client(2))
        );
        assert!(fixture.requests(0).iter().all(|(_, params)| {
            params["_meta"]["harness_serena_client"] == json!(Fixture::client(1))
        }));
        assert!(fixture.requests(1).iter().all(|(_, params)| {
            params["_meta"]["harness_serena_client"] == json!(Fixture::client(2))
        }));
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn client_routing_stays_ordered_behind_its_own_slow_request() {
        let (fixture, pool) = Fixture::new("perclient", 3, 300);
        let alpha = fixture.project("pc-alpha");
        let beta = fixture.project("pc-beta");
        let hold = Signal::new();
        fixture.plan(
            &alpha,
            Plan {
                request_hold: Some(Arc::clone(&hold)),
                ..Plan::default()
            },
        );
        connect(&pool, &Fixture::client(1), &alpha).unwrap();
        let pool = Arc::new(pool);
        let slow = spawn_rpc(
            &pool,
            &Fixture::client(1),
            &alpha,
            "tools/list",
            json!({"slow": true}),
        );
        assert!(fixture.wait_requests(0, 1));
        let activation = {
            let pool = Arc::clone(&pool);
            let beta_project = beta.clone();
            let beta_value = beta_project.to_string_lossy().into_owned();
            std::thread::spawn(move || {
                rpc(
                    &pool,
                    &Fixture::client(1),
                    &beta_project,
                    "tools/call",
                    json!({"name": "activate_project", "arguments": {"project": beta_value}}),
                )
            })
        };
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            fixture.starts.load(Ordering::SeqCst),
            1,
            "one client's routing operations stay ordered behind its own request"
        );
        hold.set();
        let slow = slow.join().unwrap().unwrap();
        assert_eq!(content(&slow)["fixture"], 0);
        let activation = activation.join().unwrap().unwrap();
        assert_eq!(activation["tools_changed"], true);
        let canonical = crate::dependency_package::resolved(&beta).unwrap();
        assert_eq!(activation["route"]["project"], json!(canonical));
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 2);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn all_busy_capacity_reports_deadline_without_evicting_protected_work() {
        let (fixture, pool) = Fixture::new("busy", 1, 300);
        let alpha = fixture.project("busy-alpha");
        let beta = fixture.project("busy-beta");
        let hold = Signal::new();
        fixture.plan(
            &alpha,
            Plan {
                request_hold: Some(Arc::clone(&hold)),
                ..Plan::default()
            },
        );
        connect(&pool, &Fixture::client(1), &alpha).unwrap();
        let pool = Arc::new(pool);
        let busy = spawn_rpc(&pool, &Fixture::client(1), &alpha, "tools/list", json!({}));
        assert!(fixture.wait_requests(0, 1));
        assert_eq!(pool.status()["counts"]["reserved"], 1);
        let deadline = Deadline::after(Duration::from_millis(300)).unwrap();
        let error = rpc_with(
            &pool,
            &Fixture::client(2),
            &beta,
            "tools/list",
            json!({"late": true}),
            deadline,
            &Cancellation::default(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut, "{error}");
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        assert!(!fixture.closed(0), "a protected request is never evicted");
        hold.set();
        busy.join().unwrap().unwrap();
        assert_eq!(
            fixture.states().len(),
            1,
            "an expired request never starts a worker later"
        );
        assert!(
            fixture
                .requests(0)
                .iter()
                .all(|(_, params)| params.get("late").is_none())
        );
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn cancellation_while_waiting_never_executes() {
        let (fixture, pool) = Fixture::new("cancel", 1, 300);
        let alpha = fixture.project("cancel-alpha");
        let beta = fixture.project("cancel-beta");
        let hold = Signal::new();
        fixture.plan(
            &alpha,
            Plan {
                request_hold: Some(Arc::clone(&hold)),
                ..Plan::default()
            },
        );
        connect(&pool, &Fixture::client(1), &alpha).unwrap();
        let pool = Arc::new(pool);
        let busy = spawn_rpc(&pool, &Fixture::client(1), &alpha, "tools/list", json!({}));
        assert!(fixture.wait_requests(0, 1));
        let cancel = Cancellation::default();
        let caller_cancel = cancel.clone();
        let caller_pool = Arc::clone(&pool);
        let caller = std::thread::spawn(move || {
            rpc_with(
                &caller_pool,
                &Fixture::client(2),
                &beta,
                "tools/list",
                json!({"late": true}),
                Fixture::deadline(),
                &caller_cancel,
            )
        });
        std::thread::sleep(Duration::from_millis(200));
        cancel.cancel();
        let error = caller.join().unwrap().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted, "{error}");
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
        hold.set();
        busy.join().unwrap().unwrap();
        assert_eq!(fixture.states().len(), 1);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn expired_requests_never_execute() {
        let (fixture, pool) = Fixture::new("gated", 3, 300);
        let project = fixture.project("gated-project");
        let hold = Signal::new();
        fixture.plan(
            &project,
            Plan {
                request_hold: Some(Arc::clone(&hold)),
                ..Plan::default()
            },
        );
        connect(&pool, &Fixture::client(1), &project).unwrap();
        connect(&pool, &Fixture::client(2), &project).unwrap();
        let pool = Arc::new(pool);
        let first = spawn_rpc(
            &pool,
            &Fixture::client(1),
            &project,
            "tools/list",
            json!({"first": true}),
        );
        assert!(fixture.wait_requests(0, 1));
        // Waiting for the same serialized worker respects the deadline.
        let deadline = Deadline::after(Duration::from_millis(300)).unwrap();
        let error = rpc_with(
            &pool,
            &Fixture::client(2),
            &project,
            "tools/list",
            json!({"second": true}),
            deadline,
            &Cancellation::default(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut, "{error}");
        // An already expired request is refused before it reaches the pool.
        let expired = Deadline::after(Duration::ZERO).unwrap();
        assert!(
            rpc_with(
                &pool,
                &Fixture::client(2),
                &project,
                "tools/list",
                json!({"third": true}),
                expired,
                &Cancellation::default(),
            )
            .is_err()
        );
        hold.set();
        first.join().unwrap().unwrap();
        assert_eq!(
            fixture.requests(0).len(),
            1,
            "an expired request must never execute later"
        );
        assert_eq!(fixture.state(0).lock().unwrap().peak_active, 1);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn failed_startup_is_shared_and_releases_capacity() {
        let (fixture, pool) = Fixture::new("failed", 1, 300);
        let broken = fixture.project("failed-project");
        let hold = Signal::new();
        fixture.plan(
            &broken,
            Plan {
                start_hold: Some(Arc::clone(&hold)),
                start_failure: true,
                ..Plan::default()
            },
        );
        let pool = Arc::new(pool);
        let first = spawn_rpc(&pool, &Fixture::client(1), &broken, "tools/list", json!({}));
        assert!(wait_until(WAIT, || fixture.starts.load(Ordering::SeqCst) == 1));
        let second = spawn_rpc(&pool, &Fixture::client(2), &broken, "tools/list", json!({}));
        std::thread::sleep(Duration::from_millis(500));
        hold.set();
        for thread in [first, second] {
            assert!(thread.join().unwrap().is_err());
        }
        assert_eq!(
            fixture.starts.load(Ordering::SeqCst),
            1,
            "compatible admissions share one startup outcome"
        );
        assert_eq!(fixture.states().len(), 0, "a failed startup owns no worker");
        let status = pool.status();
        assert_eq!(status["counters"]["startup_failures"], 1);
        assert_eq!(status["counts"]["starting"], 0);
        assert_eq!(status["counts"]["active"], 0);
        // Capacity was released: a working project starts afterwards.
        let working = fixture.project("working-project");
        let response = connect(&pool, &Fixture::client(3), &working).unwrap();
        assert_eq!(response["message"]["result"]["fixture"], 1);
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 2);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn slow_retirement_keeps_capacity_bounded_and_warm_workers_serving() {
        let (fixture, pool) = Fixture::new("retire", 2, 300);
        let alpha = fixture.project("retire-alpha");
        let beta = fixture.project("retire-beta");
        let gamma = fixture.project("retire-gamma");
        let close_hold = Signal::new();
        fixture.plan(
            &alpha,
            Plan {
                close_hold: Some(Arc::clone(&close_hold)),
                ..Plan::default()
            },
        );
        connect(&pool, &Fixture::client(1), &alpha).unwrap();
        connect(&pool, &Fixture::client(3), &gamma).unwrap();
        rpc(&pool, &Fixture::client(3), &gamma, "tools/list", json!({})).unwrap();
        let pool = Arc::new(pool);
        let pending = spawn_rpc(&pool, &Fixture::client(2), &beta, "tools/list", json!({}));
        assert!(wait_until(WAIT, || pool.status()["counts"]["retiring"] == json!(1)));
        assert_eq!(
            fixture.starts.load(Ordering::SeqCst),
            2,
            "the replacement waits for the retired generation's ownership release"
        );
        assert_eq!(pool.status()["counters"]["evictions"], 1);
        assert!(!fixture.closed(0));
        let warm = rpc(&pool, &Fixture::client(3), &gamma, "tools/list", json!({})).unwrap();
        assert_eq!(content(&warm)["fixture"], 1);
        close_hold.set();
        let response = pending.join().unwrap().unwrap();
        assert_eq!(content(&response)["fixture"], 2);
        assert!(fixture.closed(0));
        assert_eq!(pool.status()["counts"]["retiring"], 0);
        assert_eq!(pool.status()["counts"]["active"], 2);
        assert_eq!(fixture.starts.load(Ordering::SeqCst), 3);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn shutdown_reports_unreleased_ownership_and_then_completes() {
        let (fixture, pool) = Fixture::new("shutdown", 1, 300);
        let project = fixture.project("shutdown-project");
        let close_hold = Signal::new();
        fixture.plan(
            &project,
            Plan {
                close_hold: Some(Arc::clone(&close_hold)),
                ..Plan::default()
            },
        );
        connect(&pool, &Fixture::client(1), &project).unwrap();
        let pool = Arc::new(pool);
        let shutdown = {
            let pool = Arc::clone(&pool);
            std::thread::spawn(move || {
                pool.close(Deadline::after(Duration::from_millis(300)).unwrap())
            })
        };
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            !shutdown.is_finished(),
            "an unconfirmed close must not report success"
        );
        assert!(!fixture.closed(0));
        close_hold.set();
        let error = shutdown.join().unwrap().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut, "{error}");
        // Ownership was released meanwhile; a second bounded close completes.
        pool.close(Deadline::after(Duration::from_secs(5)).unwrap())
            .unwrap();
        assert!(fixture.closed(0));
        assert_eq!(pool.worker_count(), 0);
    }

    #[test]
    fn shutdown_during_startup_leaves_no_untracked_worker() {
        let (fixture, pool) = Fixture::new("shutdown-start", 1, 300);
        let project = fixture.project("start-project");
        let hold = Signal::new();
        fixture.plan(
            &project,
            Plan {
                start_hold: Some(Arc::clone(&hold)),
                ..Plan::default()
            },
        );
        let pool = Arc::new(pool);
        let pending = spawn_rpc(
            &pool,
            &Fixture::client(1),
            &project,
            "tools/list",
            json!({}),
        );
        assert!(wait_until(WAIT, || fixture.starts.load(Ordering::SeqCst) == 1));
        let shutdown = {
            let pool = Arc::clone(&pool);
            std::thread::spawn(move || {
                pool.close(Deadline::after(Duration::from_millis(600)).unwrap())
            })
        };
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            !shutdown.is_finished(),
            "close waits for the in-flight startup to release ownership"
        );
        hold.set();
        // The started worker is closed by its starter instead of being
        // published, so no owned tree survives the shutdown.
        shutdown.join().unwrap().unwrap();
        let pending = pending.join().unwrap().unwrap_err();
        assert!(pending.to_string().contains("shutting down"), "{pending}");
        assert!(wait_until(WAIT, || fixture.closed(0)));
        assert_eq!(fixture.states().len(), 1);
        assert_eq!(pool.worker_count(), 0);
        pool.close(Deadline::after(Duration::from_secs(5)).unwrap())
            .unwrap();
    }

    #[test]
    fn four_roots_cycle_records_real_evictions_and_cold_starts() {
        let (fixture, pool) = Fixture::new("cycle", 3, 300);
        let projects: Vec<PathBuf> = (0..4)
            .map(|index| fixture.project(&format!("cycle-{index}")))
            .collect();
        for (index, project) in projects.iter().enumerate() {
            connect(&pool, &Fixture::client(index + 1), project).unwrap();
        }
        let status = pool.status();
        assert_eq!(status["counters"]["cold_starts"], 4);
        assert_eq!(status["counters"]["evictions"], 1);
        assert_eq!(status["counts"]["active"], 3);
        assert_eq!(status["counts"]["limit"], 3);
        assert_eq!(fixture.closed_count(), 1);
        // Revisiting the evicted root records a real cold start instead of
        // claiming all roots stayed warm.
        connect(&pool, &Fixture::client(5), &projects[0]).unwrap();
        let status = pool.status();
        assert_eq!(status["counters"]["cold_starts"], 5);
        assert_eq!(status["counters"]["evictions"], 2);
        assert_eq!(status["counts"]["active"], 3);
        assert_eq!(fixture.closed_count(), 2);
        pool.close(Fixture::deadline()).unwrap();
    }

    #[test]
    fn status_reports_bounded_evidence_and_reset_identity() {
        let (fixture, pool) = Fixture::new("status", 3, 300);
        let project = fixture.project("status-project");
        connect(&pool, &Fixture::client(1), &project).unwrap();
        rpc(
            &pool,
            &Fixture::client(1),
            &project,
            "tools/list",
            json!({}),
        )
        .unwrap();
        let status = pool.status();
        assert_eq!(status["counters"]["hits"], 1);
        assert_eq!(status["counters"]["cold_starts"], 1);
        assert_eq!(status["counters"]["evictions"], 0);
        assert_eq!(status["counters"]["startup_failures"], 0);
        assert_eq!(status["counts"]["active"], 1);
        assert_eq!(status["counts"]["reserved"], 0);
        assert_eq!(status["counts"]["idle"], 1);
        assert_eq!(status["counts"]["starting"], 0);
        assert_eq!(status["counts"]["retiring"], 0);
        assert!(status["durations"]["queue"]["count"].as_u64().unwrap() >= 2);
        assert!(status["durations"]["start"]["count"].as_u64().unwrap() >= 1);
        assert!(status["durations"]["request"]["count"].as_u64().unwrap() >= 2);
        let identity = status["interval"]["identity"].as_str().unwrap().to_owned();
        assert_eq!(identity.len(), 64, "{identity}");
        assert!(status["interval"]["started_unix_ms"].as_u64().unwrap() > 0);
        assert_eq!(status["memory"]["observation"], "unavailable");
        assert_eq!(status["memory"]["observed_bytes"], Value::Null);
        assert_eq!(
            status["memory"]["configured_limit_bytes"],
            json!(4096u64 * 1024 * 1024)
        );
        drop(pool);
        let (_second_fixture, second) = Fixture::new("status-2", 3, 300);
        assert_ne!(
            second.status()["interval"]["identity"],
            json!(identity),
            "a restarted pool identifies its own observation interval"
        );
    }
}
