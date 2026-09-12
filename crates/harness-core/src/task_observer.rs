//! Native task observations only; transport timeouts never mean task failure.
use crate::{
    broker_state::BrokerRoot,
    task_control::ControlConnection,
    task_runtime::{read_json, save},
    task_view::{Snapshot, Watch},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const POLL: Duration = Duration::from_millis(200);

pub(crate) fn observe(
    root: &BrokerRoot,
    connection: &mut ControlConnection,
    mut initial_events: VecDeque<Value>,
    executable: &Path,
) -> io::Result<()> {
    let mut tasks = BTreeMap::<String, Value>::new();
    let mut pending = BTreeMap::<u64, (String, bool)>::new();
    let mut subscribed = BTreeSet::<String>::new();
    let mut items = BTreeMap::<String, BTreeMap<String, Value>>::new();
    let mut failures = BTreeMap::<String, Value>::new();
    let mut next = 1u64;
    let user = crate::process_service::current_user()?;
    let mut visibility = Visibility::default();
    let mut handoff = crate::task_handoff::Handoff::load(root)?;
    loop {
        if root.path().join("stop.json").try_exists()? {
            save(
                &root.path().join("closed.json"),
                &json!({"schema":1,"reason":"explicit stop","stopped":true}),
            )?;
            return Ok(());
        }
        let value = match initial_events.pop_front() {
            Some(value) => Some(value),
            None => connection.receive(POLL)?,
        };
        if let Some(value) = value {
            let mut request = None;
            let mut changed = false;
            if value.get("method").is_none()
                && let Some(id) = value["id"].as_u64()
            {
                if let Some((thread, resumed)) = pending.remove(&id) {
                    changed = true;
                    if value.get("error").is_some() {
                        save(&root.path().join("observation-error.json"), &value)?;
                        // Preserve uncertainty. An unsuccessful read cannot close a task.
                        tasks.entry(thread).or_insert(json!({}))["status"] =
                            json!({"type":"unknown"});
                    } else {
                        if value["result"]["thread"]["id"] != thread {
                            return Err(io::Error::other(
                                "native thread response identity differs",
                            ));
                        }
                        tasks.insert(thread.clone(), value["result"]["thread"].clone());
                        if resumed {
                            request = Some((thread, false));
                        }
                    }
                }
            } else {
                let params = &value["params"];
                match value["method"].as_str() {
                    Some("thread/started") => {
                        // The native TUI creates ephemeral system threads for
                        // titles. They are not user work and cannot be resumed
                        // with full history. Subscribe only after a real turn
                        // starts, when the native rollout is resumable.
                        if params["thread"]["ephemeral"] != true
                            && let Some(id) = params["thread"]["id"].as_str()
                        {
                            tasks.insert(id.into(), params["thread"].clone());
                            changed = true;
                        }
                    }
                    Some("thread/status/changed") => {
                        if let Some(id) = params["threadId"].as_str()
                            && let Some(thread) = tasks.get_mut(id)
                        {
                            thread["status"] = params["status"].clone();
                            changed = true;
                            if params["status"]["type"] == "active" && subscribed.insert(id.into())
                            {
                                request = Some((id.into(), true));
                            }
                        }
                    }
                    Some("item/completed") => {
                        if let Some(thread) = params["threadId"].as_str()
                            && tasks.contains_key(thread)
                            && let Some(id) = params["item"]["id"].as_str()
                            && matches!(
                                params["item"]["type"].as_str(),
                                Some(
                                    "userMessage"
                                        | "agentMessage"
                                        | "commandExecution"
                                        | "fileChange"
                                        | "subAgentActivity"
                                )
                            )
                        {
                            items
                                .entry(thread.into())
                                .or_default()
                                .insert(id.into(), params["item"].clone());
                            changed = true;
                        }
                    }
                    Some("turn/completed") => {
                        if let Some(id) = params["threadId"].as_str()
                            && tasks.contains_key(id)
                        {
                            request = Some((id.into(), false));
                        }
                    }
                    _ => (),
                }
            }
            if let Some((thread, resume)) = request
                && !pending
                    .values()
                    .any(|(id, kind)| id == &thread && kind == &resume)
            {
                next += 1;
                let (method, params) = if resume {
                    ("thread/resume", json!({"threadId":thread}))
                } else {
                    (
                        "thread/read",
                        json!({"threadId":thread,"includeTurns":true}),
                    )
                };
                connection.send(
                    &json!({"id":next,"method":method,"params":params}),
                    Duration::from_secs(5),
                )?;
                pending.insert(next, (thread, resume));
            }
            if changed {
                save(
                    &root.path().join("task.json"),
                    &json!({"schema":1,"threads":tasks,"completedItems":items}),
                )?;
            }
            if let Some(failure) = crate::task_failure::TurnFailure::from_event(&value)
                && tasks.contains_key(&failure.thread_id)
            {
                failures.insert(
                    failure.thread_id.clone(),
                    json!({"observedAtUnixMs":SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_millis(),"failure":failure}),
                );
                save(
                    &root.path().join("failures.json"),
                    &json!({"schema":1,"threads":failures}),
                )?;
            }
            visibility.event(root, &value, &tasks)?;
            handoff.event(root, connection, &value)?;
            if let Some(id) = handoff.attached_thread() {
                subscribed.insert(id.into());
            }
        }
        visibility.tick(root, connection, executable, &user, &tasks)?;
        let settled = tasks
            .iter()
            .filter(|(id, thread)| {
                thread_settled(thread, failures.get(*id))
                    && visibility.thread_settled(id)
                    && !pending.values().any(|(requested, _)| requested == *id)
            })
            .map(|(id, _)| id.clone())
            .collect();
        let visible = tasks
            .keys()
            .filter(|id| visibility.visible_for(id))
            .cloned()
            .collect();
        handoff.tick(root, connection, &tasks, &failures, &settled, &visible)?;
        if root.path().join("client-closed.json").try_exists()?
            && pending.is_empty()
            && visibility.settled()
            && handoff.settled()
            && tasks
                .iter()
                .all(|(id, thread)| thread_settled(thread, failures.get(id)))
        {
            save(
                &root.path().join("closed.json"),
                &json!({"schema":1,"reason":"client closed and observed work settled"}),
            )?;
            return Ok(());
        }
    }
}

fn thread_settled(thread: &Value, failure: Option<&Value>) -> bool {
    match thread["status"]["type"].as_str() {
        Some("idle" | "notLoaded") => true,
        Some("systemError") => {
            // A prior failure must not settle a newer or still-unknown turn.
            let Some(turn) = thread["turns"].as_array().and_then(|turns| turns.last()) else {
                return false;
            };
            turn["status"] == "failed"
                && turn["id"].as_str().is_some_and(|id| {
                    failure.and_then(|saved| saved["failure"]["turnId"].as_str()) == Some(id)
                })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{Visibility, thread_settled};
    use serde_json::json;

    #[test]
    fn terminal_error_can_settle_after_observation_but_stale_failure_cannot() {
        let mut thread = json!({"status":{"type":"systemError"},"turns":[{"id":"refused-turn","status":"failed"}]});
        let saved = json!({"failure":{"turnId":"refused-turn"}});
        assert!(!thread_settled(&thread, None));
        assert!(thread_settled(&thread, Some(&saved)));
        thread["turns"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"new-turn","status":"failed"}));
        assert!(!thread_settled(&thread, Some(&saved)));
        thread["status"]["type"] = json!("active");
        assert!(!thread_settled(&thread, Some(&saved)));
    }

    #[test]
    fn a_known_leader_view_cannot_stand_in_for_another_conversation() {
        let mut views = Visibility {
            primary: "previous".into(),
            ..Visibility::default()
        };
        views.conversations.insert("previous".into(), true);
        assert!(views.visible_for("previous"));
        assert!(!views.visible_for("successor"));
        assert!(!views.visible_for("unmapped-worker"));
        views.conversations.insert("previous".into(), false);
        views.conversations.insert("successor".into(), true);
        assert!(views.visible_for("successor"));
        assert!(!views.visible_for("previous"));
    }
}

#[derive(Serialize)]
enum ViewAction {
    Interrupt { thread: String, turn: String },
    Continue { thread: String },
}

impl ViewAction {
    fn thread(&self) -> &str {
        match self {
            Self::Interrupt { thread, .. } | Self::Continue { thread } => thread,
        }
    }
}

#[derive(Default)]
struct Visibility {
    snapshots: BTreeMap<String, Snapshot>,
    watches: BTreeMap<String, Watch>,
    conversations: BTreeMap<String, bool>,
    primary: String,
    visible: Option<bool>,
    active: BTreeMap<String, String>,
    operations: BTreeMap<String, BTreeSet<String>>,
    stopping: BTreeMap<String, String>,
    recovery: BTreeSet<String>,
    pending: BTreeMap<String, ViewAction>,
    next: u64,
}

impl Visibility {
    fn save(&self, root: &BrokerRoot) -> io::Result<()> {
        save(
            &root.path().join("visibility.json"),
            &json!({"schema":1,
                "visible":self.visible.unwrap_or(false), "conversations":self.conversations, "activeTurns":self.active, "activeOperations":self.operations,
            "interruptionRequested":self.stopping, "recoveryPending":self.recovery,
            "pendingRequests":self.pending}),
        )
    }

    fn settled(&self) -> bool {
        self.active.is_empty() && self.operations.is_empty() && self.pending.is_empty()
    }

    fn thread_settled(&self, thread: &str) -> bool {
        !self.active.contains_key(thread)
            && !self.operations.contains_key(thread)
            && !self
                .pending
                .values()
                .any(|action| action.thread() == thread)
    }

    fn visible_for(&self, thread: &str) -> bool {
        self.conversations
            .get(thread)
            .copied()
            .unwrap_or_else(|| self.primary.is_empty() && self.conversations.get("") == Some(&true))
    }

    fn event(
        &mut self,
        root: &BrokerRoot,
        event: &Value,
        tasks: &BTreeMap<String, Value>,
    ) -> io::Result<()> {
        let mut changed = false;
        if event.get("method").is_none()
            && let Some(id) = event["id"].as_str()
            && let Some(action) = self.pending.remove(id)
        {
            if event.get("error").is_some() {
                save(&root.path().join("visibility-error.json"), event)?;
                return Err(io::Error::other(
                    "native visibility control failed; reconcile saved work",
                ));
            }
            if matches!(action, ViewAction::Continue { .. })
                && !event["result"]["turn"]["id"].is_string()
            {
                return Err(io::Error::other(
                    "native continuation returned no turn identity",
                ));
            }
            changed = true;
        }
        let params = &event["params"];
        if let (Some(thread), Some(item)) =
            (params["threadId"].as_str(), params["item"]["id"].as_str())
            && tasks.contains_key(thread)
            && matches!(
                params["item"]["type"].as_str(),
                Some("commandExecution" | "fileChange" | "mcpToolCall" | "dynamicToolCall")
            )
        {
            match event["method"].as_str() {
                Some("item/started") => {
                    changed |= self
                        .operations
                        .entry(thread.into())
                        .or_default()
                        .insert(item.into());
                }
                Some("item/completed") => {
                    if let Some(operations) = self.operations.get_mut(thread) {
                        changed |= operations.remove(item);
                        if operations.is_empty() {
                            self.operations.remove(thread);
                        }
                    }
                }
                _ => (),
            }
        }
        if let (Some(thread), Some(turn)) =
            (params["threadId"].as_str(), params["turn"]["id"].as_str())
            && tasks.contains_key(thread)
        {
            match event["method"].as_str() {
                Some("turn/started") => {
                    self.active.insert(thread.into(), turn.into());
                    changed = true;
                }
                Some("turn/completed") => {
                    if self.active.get(thread).is_some_and(|active| active == turn) {
                        self.active.remove(thread);
                    }
                    if self
                        .stopping
                        .get(thread)
                        .is_some_and(|stopping| stopping == turn)
                    {
                        self.stopping.remove(thread);
                        if params["turn"]["status"] != "interrupted" {
                            // Completion or a provider failure is not an interrupted
                            // visibility attempt and must not trigger automatic replay.
                            self.recovery.remove(thread);
                        }
                    }
                    changed = true;
                }
                _ => (),
            }
        }
        if changed {
            self.save(root)?;
        }
        Ok(())
    }

    fn tick(
        &mut self,
        root: &BrokerRoot,
        connection: &mut ControlConnection,
        executable: &Path,
        user: &str,
        tasks: &BTreeMap<String, Value>,
    ) -> io::Result<()> {
        let mut snapshots = BTreeMap::new();
        match read_json::<Value>(&root.path().join("view.json")) {
            Ok(value) => {
                if value["schema"] != 1 {
                    return Err(io::Error::other("unsupported conversation view record"));
                }
                self.primary = value["threadId"].as_str().unwrap_or_default().to_owned();
                snapshots.insert(
                    self.primary.clone(),
                    serde_json::from_value::<Snapshot>(value["window"].clone())?,
                );
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
        match read_json::<Value>(&root.path().join("additional-views.json")) {
            Ok(value) => {
                if value["schema"] != 1 {
                    return Err(io::Error::other("unsupported additional view record"));
                }
                for (id, snapshot) in
                    serde_json::from_value::<BTreeMap<String, Snapshot>>(value["threads"].clone())?
                {
                    if snapshots.insert(id, snapshot).is_some() {
                        return Err(io::Error::other("duplicate conversation view identity"));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
        self.watches.retain(|id, _| snapshots.contains_key(id));
        for (id, snapshot) in &snapshots {
            if self.snapshots.get(id) != Some(snapshot) {
                self.watches.remove(id);
                if let Some(watch) = Watch::open(snapshot, executable, user)? {
                    self.watches.insert(id.clone(), watch);
                }
            }
        }
        self.snapshots = snapshots;
        let mut conversations = BTreeMap::new();
        for id in self.snapshots.keys() {
            conversations.insert(
                id.clone(),
                match self.watches.get(id) {
                    Some(watch) => watch.visible()?,
                    None => false,
                },
            );
        }
        let changed = conversations != self.conversations;
        self.conversations = conversations;
        let required = self
            .active
            .keys()
            .chain(self.recovery.iter())
            .collect::<BTreeSet<_>>();
        let visible = if required.is_empty() {
            self.visible_for(&self.primary)
        } else {
            required.iter().all(|id| self.visible_for(id))
        };
        if self.visible != Some(visible) || changed {
            self.visible = Some(visible);
            self.save(root)?;
        }
        for (thread, turn) in self.active.clone() {
            if self.visible_for(&thread) || self.stopping.get(&thread) == Some(&turn) {
                continue;
            }
            self.stopping.insert(thread.clone(), turn.clone());
            self.recovery.insert(thread.clone());
            self.send(
                root,
                connection,
                "turn/interrupt",
                json!({"threadId":thread,"turnId":turn}),
                ViewAction::Interrupt { thread, turn },
            )?;
        }
        for thread in self.recovery.clone() {
            if !self.visible_for(&thread)
                || !self.thread_settled(&thread)
                || tasks
                    .get(&thread)
                    .is_none_or(|task| task["status"]["type"] != "idle")
            {
                continue;
            }
            self.recovery.remove(&thread);
            self.send(root, connection, "turn/start", json!({"threadId":thread,"input":[{"type":"text",
                "text":"Your conversation window is visible again. Continue only the previously authorized task. First reconcile interrupted tool operations and existing artifacts; preserve completed work and do not blindly repeat effects with an uncertain outcome."}]}),
                ViewAction::Continue {thread})?;
        }
        Ok(())
    }

    fn send(
        &mut self,
        root: &BrokerRoot,
        connection: &mut ControlConnection,
        method: &str,
        params: Value,
        action: ViewAction,
    ) -> io::Result<()> {
        self.next += 1;
        let id = format!("visibility-{}", self.next);
        self.pending.insert(id.clone(), action);
        self.save(root)?;
        connection.send(
            &json!({"id":id,"method":method,"params":params}),
            Duration::from_secs(5),
        )
    }
}
