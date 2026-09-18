//! Per-request admission against observed native attempts and live conversation
//! windows. Request metadata supplies correlation only, never its own authority.
use crate::{
    broker_state::BrokerRoot,
    process::{Cancellation, Deadline},
    task_forward::{ForwardRequest, Forwarder},
    task_request::RequestIdentity,
    task_runtime::read_json,
    task_view::{Snapshot, Watch},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    Ready,
    Waiting(&'static str),
    Rejected(&'static str),
}

pub struct Gate {
    executable: PathBuf,
    user: String,
    watches: BTreeMap<String, (Snapshot, Option<Watch>)>,
}

impl Gate {
    pub fn new(executable: &Path) -> io::Result<Self> {
        Ok(Self {
            executable: executable.into(),
            user: crate::process_service::current_user()?,
            watches: BTreeMap::new(),
        })
    }

    pub fn check(&mut self, root: &BrokerRoot, request: &RequestIdentity) -> io::Result<Admission> {
        if root.path().join("stop.json").try_exists()?
            || root.path().join("closed.json").try_exists()?
        {
            return Ok(Admission::Rejected("task is stopped"));
        }
        let tasks = optional(&root.path().join("task.json"))?;
        let visibility = optional(&root.path().join("visibility.json"))?;
        let admission = observed_attempt(request, &tasks, &visibility);
        if admission != Admission::Ready {
            return Ok(admission);
        }
        let initial = optional(&root.path().join("view.json"))?;
        let additional = optional(&root.path().join("additional-views.json"))?;
        let snapshot = if initial["threadId"] == request.thread {
            &initial["window"]
        } else {
            &additional["threads"][&request.thread]
        };
        if snapshot.is_null() {
            return Ok(Admission::Waiting("conversation has no registered view"));
        }
        let snapshot: Snapshot = serde_json::from_value(snapshot.clone())?;
        if self
            .watches
            .get(&request.thread)
            .is_none_or(|(old, _)| old != &snapshot)
        {
            let watch = Watch::open(&snapshot, &self.executable, &self.user)?;
            self.watches
                .insert(request.thread.clone(), (snapshot, watch));
        }
        let visible = self.watches[&request.thread]
            .1
            .as_ref()
            .map(Watch::visible)
            .transpose()?
            .unwrap_or(false);
        if !visible {
            return Ok(Admission::Waiting("conversation view is unavailable"));
        }
        // Recheck observed ownership and stop state after inspecting the actual
        // window. A historical visible=true receipt is never an admission input.
        if root.path().join("stop.json").try_exists()?
            || root.path().join("closed.json").try_exists()?
        {
            return Ok(Admission::Rejected("task is stopped"));
        }
        Ok(observed_attempt(
            request,
            &optional(&root.path().join("task.json"))?,
            &optional(&root.path().join("visibility.json"))?,
        ))
    }

    /// Wait without model calls, then use the same unmodified request bytes for
    /// one forwarding attempt. Caller owns upstream identity and cancellation.
    pub fn forward(
        &mut self,
        forwarder: &Forwarder,
        root: &BrokerRoot,
        request: ForwardRequest<'_>,
        deadline: Deadline,
        cancel: &Cancellation,
        output: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        let body: Value = serde_json::from_slice(request.body)?;
        let identity = RequestIdentity::from_request(&body).map_err(io::Error::other)?;
        loop {
            if cancel.is_cancelled() || deadline.expired() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "request admission cancelled or observation deadline expired",
                ));
            }
            match self.check(root, &identity)? {
                Admission::Ready => {
                    return forwarder.forward(root, request, deadline, cancel, output);
                }
                Admission::Rejected(reason) => {
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, reason));
                }
                Admission::Waiting(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}

fn optional(path: &Path) -> io::Result<Value> {
    match read_json(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Value::Null),
        result => result,
    }
}

fn observed_attempt(request: &RequestIdentity, tasks: &Value, visibility: &Value) -> Admission {
    if request.kind != "turn" {
        return Admission::Rejected("request kind needs explicit visible support");
    }
    let thread = &tasks["threads"][&request.thread];
    if thread.is_null() {
        return Admission::Waiting("native thread has not been observed");
    }
    if thread["pendingNativeRead"] == true {
        return Admission::Waiting("native child identity is being read");
    }
    if thread["id"] != request.thread
        || thread["sessionId"] != request.session
        || !thread["ephemeral"].is_boolean()
    {
        return Admission::Rejected("native thread identity or scope differs");
    }
    if visibility["activeTurns"][&request.thread] != request.turn {
        return Admission::Waiting("native attempt has not been reconciled");
    }
    if !visibility["interruptionRequested"][&request.thread].is_null() {
        return Admission::Waiting("native attempt is being interrupted");
    }
    Admission::Ready
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parent_visibility_and_stale_attempts_do_not_cover_a_child() {
        let request = RequestIdentity {
            thread: "child".into(),
            session: "parent".into(),
            turn: "current".into(),
            kind: "turn".into(),
        };
        let tasks =
            json!({"threads":{"child":{"id":"child","sessionId":"parent","ephemeral":false}}});
        let mut visibility = json!({"visible":true,"activeTurns":{"parent":"current"},"conversations":{"parent":true}});
        assert!(matches!(
            observed_attempt(&request, &tasks, &visibility),
            Admission::Waiting(_)
        ));
        visibility["activeTurns"]["child"] = json!("old");
        assert!(matches!(
            observed_attempt(&request, &tasks, &visibility),
            Admission::Waiting(_)
        ));
        visibility["activeTurns"]["child"] = json!("current");
        assert_eq!(
            observed_attempt(&request, &tasks, &visibility),
            Admission::Ready
        );
        visibility["interruptionRequested"] = json!({"child":"current"});
        assert!(matches!(
            observed_attempt(&request, &tasks, &visibility),
            Admission::Waiting(_)
        ));
    }

    #[test]
    fn an_ephemeral_helper_is_ready_only_with_its_own_matching_identity() {
        let request = RequestIdentity {
            thread: "helper".into(),
            session: "parent".into(),
            turn: "current".into(),
            kind: "turn".into(),
        };
        let tasks =
            json!({"threads":{"helper":{"id":"helper","sessionId":"parent","ephemeral":true}}});
        let visibility = json!({"visible":true,"activeTurns":{"helper":"current"},"conversations":{"helper":true}});
        assert_eq!(
            observed_attempt(&request, &tasks, &visibility),
            Admission::Ready
        );
        let mismatched =
            json!({"threads":{"helper":{"id":"helper","sessionId":"other","ephemeral":true}}});
        assert!(matches!(
            observed_attempt(&request, &mismatched, &visibility),
            Admission::Rejected(_)
        ));
    }

    #[test]
    fn a_recorded_attempt_without_its_own_window_never_reaches_upstream() {
        let owned = BrokerRoot::prepare().unwrap();
        let root = owned.root();
        crate::task_runtime::save(
            &root.path().join("task.json"),
            &json!({"threads":{"child":{"id":"child","sessionId":"parent","ephemeral":false}}}),
        )
        .unwrap();
        crate::task_runtime::save(&root.path().join("visibility.json"), &json!({"visible":true,"activeTurns":{"child":"current"},"conversations":{"parent":true}})).unwrap();
        crate::task_runtime::save(
            &root.path().join("view.json"),
            &json!({"threadId":"parent","window":{}}),
        )
        .unwrap();
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!(
            "http://127.0.0.1:{}/responses",
            listener.local_addr().unwrap().port()
        );
        let metadata = json!({"thread_id":"child","session_id":"parent","turn_id":"current","request_kind":"turn"});
        let body = serde_json::to_vec(&json!({"client_metadata":{"thread_id":"child","session_id":"parent","turn_id":"current","x-codex-turn-metadata":metadata.to_string()}})).unwrap();
        let mut gate = Gate::new(Path::new("unused-native-executable")).unwrap();
        let forwarder = Forwarder::new().unwrap();
        let result = gate.forward(
            &forwarder,
            root,
            ForwardRequest {
                url: &url,
                headers: &[],
                body: &body,
            },
            Deadline::after(Duration::from_millis(150)).unwrap(),
            &Cancellation::default(),
            |_| panic!("no response before admission"),
        );
        assert!(result.is_err());
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
        );
        crate::task_runtime::save(
            &root.path().join("task.json"),
            &json!({"threads":{"child":{"id":"child","sessionId":"parent","ephemeral":true}}}),
        )
        .unwrap();
        let result = gate.forward(
            &forwarder,
            root,
            ForwardRequest {
                url: &url,
                headers: &[],
                body: &body,
            },
            Deadline::after(Duration::from_millis(150)).unwrap(),
            &Cancellation::default(),
            |_| panic!("no response before admission"),
        );
        assert!(result.is_err());
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
        );
        crate::task_runtime::save(&root.path().join("stop.json"), &json!({"stopped":true}))
            .unwrap();
        assert_eq!(
            gate.check(
                root,
                &RequestIdentity::from_request(&serde_json::from_slice(&body).unwrap()).unwrap()
            )
            .unwrap(),
            Admission::Rejected("task is stopped")
        );
    }
}
