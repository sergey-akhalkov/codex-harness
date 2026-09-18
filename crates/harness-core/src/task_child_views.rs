//! Name native child conversations and queue their own windows before admission.
use crate::{
    broker_state::BrokerRoot,
    task_control::ControlConnection,
    task_runtime::{read_json, save},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    time::Duration,
};

#[derive(Default)]
pub(crate) struct ChildViews {
    pending: BTreeMap<String, Value>,
    requested: BTreeMap<String, Value>,
    waiting: BTreeSet<String>,
}

pub(crate) fn conversation_parent(task: &Value) -> Option<&str> {
    let parent = if task["pendingNativeRead"] == true {
        task["discoveredParent"].as_str()
    } else {
        task["parentThreadId"]
            .as_str()
            .or_else(|| task["forkedFromId"].as_str())
    };
    parent.filter(|id| !id.is_empty())
}

pub(crate) fn helper_conversation(task: &Value, tasks: &BTreeMap<String, Value>) -> bool {
    task["ephemeral"] == true
        || conversation_parent(task).is_some_and(|parent| {
            tasks.get(parent).is_some_and(|owner| {
                conversation_parent(owner)
                    .is_some_and(|grandparent| tasks.contains_key(grandparent))
            })
        })
}

impl ChildViews {
    pub(crate) fn load(root: &BrokerRoot) -> io::Result<Self> {
        let requested = match read_json(&root.path().join("child-view-requests.json")) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error),
        };
        let waiting = match read_json::<Value>(&root.path().join("child-view-capacity.json")) {
            Ok(value) => serde_json::from_value(value["waiting"].clone())?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
            Err(error) => return Err(error),
        };
        Ok(Self {
            requested,
            pending: BTreeMap::new(),
            waiting,
        })
    }

    pub(crate) fn event(&mut self, root: &BrokerRoot, event: &Value) -> io::Result<Option<String>> {
        let Some(id) = event["id"]
            .as_str()
            .and_then(|id| id.strip_prefix("child-view:"))
        else {
            return Ok(None);
        };
        let Some(request) = self.pending.remove(id) else {
            return Ok(None);
        };
        if event.get("error").is_some() {
            save(&root.path().join("child-view-error.json"), event)?;
            return Err(io::Error::other(
                "native executor conversation could not be named",
            ));
        }
        self.requested.insert(id.to_owned(), request);
        save(
            &root.path().join("child-view-requests.json"),
            &json!(self.requested),
        )?;
        Ok(Some(id.to_owned()))
    }

    pub(crate) fn tick(
        &mut self,
        root: &BrokerRoot,
        connection: &mut ControlConnection,
        tasks: &BTreeMap<String, Value>,
    ) -> io::Result<()> {
        let mut waiting = BTreeSet::new();
        for (id, task) in tasks {
            let parent = conversation_parent(task);
            if self.requested.contains_key(id)
                || self.pending.contains_key(id)
                || !parent.is_some_and(|parent| tasks.contains_key(parent))
            {
                continue;
            }
            let helper = helper_conversation(task, tasks);
            let used: BTreeSet<u64> = self
                .requested
                .values()
                .chain(self.pending.values())
                .filter_map(|value| value["slot"].as_u64())
                .collect();
            let (slot, title) = if helper {
                let slot = (4u64..)
                    .find(|slot| !used.contains(slot))
                    .expect("helper slots");
                (slot, format!("Helper {}", slot - 3))
            } else {
                let Some(slot) = (1..=2).find(|slot| !used.contains(slot)) else {
                    waiting.insert(id.clone());
                    continue;
                };
                (slot, format!("Executor {slot}"))
            };
            connection.send(&json!({"id":format!("child-view:{id}"),"method":"thread/name/set","params":{"threadId":id,"name":title}}), Duration::from_secs(5))?;
            self.pending.insert(
                id.clone(),
                json!({"schema":1,"threadId":id,"title":title,"slot":slot}),
            );
        }
        if waiting != self.waiting {
            save(
                &root.path().join("child-view-capacity.json"),
                &json!({"schema":1,"waiting":waiting,"reason":"executor view capacity requires reconciliation"}),
            )?;
            self.waiting = waiting;
        }
        Ok(())
    }

    pub(crate) fn awaiting_first_view(&self, id: &str) -> bool {
        self.pending.contains_key(id)
            || self.requested.contains_key(id)
            || self.waiting.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn out_of_order_names_keep_two_executors_and_a_helper_window() {
        let owned = BrokerRoot::prepare().unwrap();
        let root = owned.root();
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server =
            std::thread::spawn(move || tungstenite::accept(listener.accept().unwrap().0).unwrap());
        let mut connection =
            ControlConnection::connect(port, &"a".repeat(64), Duration::from_secs(2)).unwrap();
        let mut server = server.join().unwrap();
        server
            .get_mut()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let tasks = BTreeMap::from([
            ("lead".into(), json!({"ephemeral":false})),
            (
                "a".into(),
                json!({"ephemeral":false,"parentThreadId":"lead"}),
            ),
            (
                "b".into(),
                json!({"ephemeral":false,"parentThreadId":"lead"}),
            ),
            (
                "helper".into(),
                json!({"ephemeral":true,"parentThreadId":"lead"}),
            ),
            (
                "orphan".into(),
                json!({"ephemeral":false,"parentThreadId":"unknown"}),
            ),
        ]);
        let mut views = ChildViews::load(root).unwrap();
        views.tick(root, &mut connection, &tasks).unwrap();
        let mut replies = Vec::new();
        let mut titles = BTreeMap::<String, String>::new();
        for _ in 0..3 {
            let request: Value =
                serde_json::from_str(server.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(request["method"], "thread/name/set");
            let id = request["params"]["threadId"].as_str().unwrap().to_owned();
            titles.insert(id, request["params"]["name"].as_str().unwrap().to_owned());
            replies.push(json!({"id":request["id"],"result":{}}));
        }
        assert_eq!(titles["a"], "Executor 1");
        assert_eq!(titles["b"], "Executor 2");
        assert_eq!(titles["helper"], "Helper 1");
        assert!(!root.path().join("child-view-requests.json").exists());
        for reply in replies.into_iter().rev() {
            views.event(root, &reply).unwrap();
        }
        let reloaded = ChildViews::load(root).unwrap();
        assert_eq!(reloaded.requested.len(), 3);
        assert_eq!(reloaded.requested["a"]["slot"], 1);
        assert_eq!(reloaded.requested["b"]["slot"], 2);
        assert_eq!(reloaded.requested["helper"]["slot"], 4);
        assert_eq!(reloaded.requested["helper"]["title"], "Helper 1");
        assert!(reloaded.awaiting_first_view("helper"));
        assert!(!reloaded.awaiting_first_view("orphan"));
        let mut over_capacity = tasks;
        over_capacity.insert(
            "c".into(),
            json!({"ephemeral":false,"parentThreadId":"lead"}),
        );
        views.tick(root, &mut connection, &over_capacity).unwrap();
        views.tick(root, &mut connection, &over_capacity).unwrap();
        assert!(views.awaiting_first_view("c"));
        assert_eq!(views.requested, reloaded.requested);
        assert!(views.pending.is_empty());
        let capacity: Value = read_json(&root.path().join("child-view-capacity.json")).unwrap();
        assert_eq!(capacity["waiting"], json!(["c"]));
        assert!(ChildViews::load(root).unwrap().awaiting_first_view("c"));
        // No third naming request was sent; existing native control remains usable.
        connection
            .send(
                &json!({"id":"still-connected","method":"owned/probe"}),
                Duration::from_secs(2),
            )
            .unwrap();
        let next: Value = serde_json::from_str(server.read().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(next["id"], "still-connected");
        over_capacity.remove("c");
        views.tick(root, &mut connection, &over_capacity).unwrap();
        assert!(!views.awaiting_first_view("c"));
        let capacity: Value = read_json(&root.path().join("child-view-capacity.json")).unwrap();
        assert_eq!(capacity["waiting"], json!([]));
    }

    #[test]
    fn a_nested_child_keeps_the_helper_slot_without_native_ephemeral() {
        let owned = BrokerRoot::prepare().unwrap();
        let root = owned.root();
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server =
            std::thread::spawn(move || tungstenite::accept(listener.accept().unwrap().0).unwrap());
        let mut connection =
            ControlConnection::connect(port, &"a".repeat(64), Duration::from_secs(2)).unwrap();
        let mut server = server.join().unwrap();
        server
            .get_mut()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let tasks = BTreeMap::from([
            ("lead".into(), json!({"ephemeral":false})),
            (
                "executor".into(),
                json!({"ephemeral":false,"parentThreadId":"lead"}),
            ),
            (
                "helper".into(),
                json!({"ephemeral":false,"parentThreadId":"executor"}),
            ),
            (
                "pending-helper".into(),
                json!({"pendingNativeRead":true,"discoveredParent":"executor"}),
            ),
            (
                "forked".into(),
                json!({"ephemeral":true,"forkedFromId":"executor"}),
            ),
        ]);
        let mut views = ChildViews::load(root).unwrap();
        views.tick(root, &mut connection, &tasks).unwrap();
        let mut titles = BTreeMap::<String, String>::new();
        let mut slots = BTreeMap::<String, u64>::new();
        for _ in 0..4 {
            let request: Value =
                serde_json::from_str(server.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(request["method"], "thread/name/set");
            let id = request["params"]["threadId"].as_str().unwrap().to_owned();
            titles.insert(
                id.clone(),
                request["params"]["name"].as_str().unwrap().to_owned(),
            );
            views
                .event(root, &json!({"id":request["id"],"result":{}}))
                .unwrap();
            slots.insert(
                id,
                views.requested[request["params"]["threadId"].as_str().unwrap()]["slot"]
                    .as_u64()
                    .unwrap(),
            );
        }
        assert_eq!(titles["executor"], "Executor 1");
        assert_eq!(slots["executor"], 1);
        assert!(titles["helper"].starts_with("Helper "));
        assert!(slots["helper"] >= 4);
        assert!(titles["pending-helper"].starts_with("Helper "));
        assert!(slots["pending-helper"] >= 4);
        assert!(titles["forked"].starts_with("Helper "));
        assert!(slots["forked"] >= 4);
        assert!(!titles.contains_key("lead"));
    }
}
