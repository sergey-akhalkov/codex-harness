//! Serialized client startup. A WMI PID receipt is not endpoint readiness.
#![cfg(windows)]
use crate::{
    broker_endpoint::{self, Endpoint, Observation},
    broker_http,
    broker_state::BrokerRoot,
    process::{Cancellation, Deadline},
    process_service,
};
use std::{collections::BTreeMap, io, path::Path, time::Duration};

#[derive(Debug, serde::Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Retirement {
    Absent,
    Pending { pid: Option<u32> },
    Exited { pid: u32, exit_code: u32 },
}

/// Retirement never starts a service or kills by PID. It authenticates against
/// the recorded owner even if source has changed, then observes that exact handle.
/// An unpublished instance and unavailable identity evidence remain untouched.
pub fn retire(
    root: &BrokerRoot,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Retirement> {
    stop(deadline, cancel)?;
    let _startup = root.startup(deadline, cancel)?;
    match broker_endpoint::observe(root)? {
        Observation::Absent | Observation::Stale => {
            let probe = root.try_instance()?;
            Ok(if probe.is_some() {
                Retirement::Absent
            } else {
                Retirement::Pending { pid: None }
            })
        }
        Observation::Ready { endpoint, owner } => {
            let result = broker_http::exchange(
                endpoint.port,
                endpoint.token(),
                "retire",
                &serde_json::json!({}),
                deadline,
                cancel,
            )?;
            if result.get("retiring").and_then(serde_json::Value::as_bool) != Some(true) {
                return Err(io::Error::other(
                    "broker did not confirm retirement; preserving owner",
                ));
            }
            while owner.is_running()? {
                if cancel.is_cancelled() {
                    stop(deadline, cancel)?;
                }
                if deadline.expired() {
                    return Ok(Retirement::Pending {
                        pid: Some(endpoint.pid),
                    });
                }
                std::thread::sleep(Duration::from_millis(20).min(deadline.remaining()));
            }
            Ok(Retirement::Exited {
                pid: endpoint.pid,
                exit_code: owner
                    .exit_code()?
                    .ok_or_else(|| io::Error::other("broker exit status unavailable"))?,
            })
        }
    }
}

fn stop(deadline: Deadline, cancel: &Cancellation) -> io::Result<()> {
    if cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "broker startup cancelled; existing owner preserved",
        ));
    }
    if deadline.expired() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "broker startup deadline elapsed; existing owner preserved",
        ));
    }
    Ok(())
}

fn healthy(
    endpoint: Endpoint,
    source: &str,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Endpoint> {
    if endpoint.source != source {
        return Err(io::Error::other(
            "broker has older source/runtime; retire it or wait for idle shutdown",
        ));
    }
    let status = broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        "status",
        &serde_json::json!({}),
        deadline,
        cancel,
    )?;
    if status.get("source").and_then(serde_json::Value::as_str) != Some(source)
        || status.get("retiring").and_then(serde_json::Value::as_bool) != Some(false)
    {
        return Err(io::Error::other(
            "broker readiness/source differs; preserving owner",
        ));
    }
    Ok(endpoint)
}

/// The caller supplies a trusted bootstrap and the source identity of its full
/// service configuration. A compatible live owner is joined, never replaced.
/// New services must publish an exact endpoint and answer authenticated status.
pub fn ensure(
    root: &BrokerRoot,
    program: &Path,
    arguments: Vec<String>,
    environment: BTreeMap<String, String>,
    source: &str,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Endpoint> {
    stop(deadline, cancel)?;
    if source.len() != 64 || !source.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "broker source identity must be a digest",
        ));
    }
    let _startup = root.startup(deadline, cancel)?;
    if let Observation::Ready {
        endpoint,
        owner: _owner,
    } = broker_endpoint::observe(root)?
    {
        return healthy(endpoint, source, deadline, cancel);
    }
    // Reconcile an exact stale receipt while this client owns both admission
    // leases, before launching and polling. Otherwise its readiness reader
    // races the new service's exclusive stale-file removal (Windows error 32).
    // A live/unknown owner still blocks the claim and is never replaced.
    let probe = broker_endpoint::Instance::claim(root)?;
    drop(probe);
    stop(deadline, cancel)?;
    let service = process_service::spawn(
        program,
        root.path(),
        arguments,
        environment,
        deadline,
        cancel,
    )?;
    loop {
        stop(deadline, cancel)?;
        if let Observation::Ready {
            endpoint,
            owner: _owner,
        } = broker_endpoint::observe(root)?
        {
            if endpoint.identity() != service.identity() {
                return Err(io::Error::other(
                    "broker endpoint does not belong to the launched service; preserving owner",
                ));
            }
            return healthy(endpoint, source, deadline, cancel);
        }
        if !service.is_running()? {
            return Err(io::Error::other(format!(
                "broker exited before endpoint readiness (exit {:?})",
                service.exit_code()?
            )));
        }
        std::thread::sleep(Duration::from_millis(20).min(deadline.remaining()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker_endpoint::Instance;
    use std::{net::TcpListener, thread};
    const SOURCE: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    fn deadline() -> Deadline {
        Deadline::after(Duration::from_secs(3)).unwrap()
    }
    #[test]
    fn stale_receipt_is_reconciled_before_bootstrap_and_readiness_polling() {
        let prepared = BrokerRoot::prepare().unwrap();
        let root = prepared.root();
        let mut instance = Instance::claim(root).unwrap();
        let endpoint = instance.publish(12345, SOURCE).unwrap();
        let mut value = serde_json::to_value(&endpoint).unwrap();
        value["creation_time"] = serde_json::json!(endpoint.creation_time - 1);
        std::fs::write(
            root.path().join("endpoint.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        drop(instance);
        assert!(matches!(
            broker_endpoint::observe(root).unwrap(),
            Observation::Stale
        ));
        let error = ensure(
            root,
            &root.path().join("missing-bootstrap.exe"),
            vec![],
            BTreeMap::new(),
            SOURCE,
            deadline(),
            &Cancellation::default(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(matches!(
            broker_endpoint::observe(root).unwrap(),
            Observation::Absent
        ));
        assert!(root.try_instance().unwrap().is_some());
    }
    #[test]
    fn held_unpublished_owner_blocks_startup_without_executable_lookup() {
        let prepared = BrokerRoot::prepare().unwrap();
        let root = prepared.root();
        let owner = Instance::claim(root).unwrap();
        let error = ensure(
            root,
            &root.path().join("missing.exe"),
            vec![],
            BTreeMap::new(),
            SOURCE,
            deadline(),
            &Cancellation::default(),
        )
        .err()
        .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(!root.path().join("endpoint.json").exists());
        assert!(matches!(
            retire(root, deadline(), &Cancellation::default()).unwrap(),
            Retirement::Pending { pid: None }
        ));
        owner.close().unwrap();
        assert!(matches!(
            retire(root, deadline(), &Cancellation::default()).unwrap(),
            Retirement::Absent
        ));
    }
    #[test]
    fn live_source_mismatch_preserves_receipt_before_contacting_port() {
        let prepared = BrokerRoot::prepare().unwrap();
        let root = prepared.root();
        let mut owner = Instance::claim(root).unwrap();
        let endpoint = owner.publish(12345, SOURCE).unwrap();
        let before = std::fs::read(root.path().join("endpoint.json")).unwrap();
        let error = ensure(
            root,
            &std::env::current_exe().unwrap(),
            vec![],
            BTreeMap::new(),
            &"3".repeat(64),
            deadline(),
            &Cancellation::default(),
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("older source/runtime"));
        assert!(!error.to_string().contains(endpoint.token()));
        assert_eq!(
            std::fs::read(root.path().join("endpoint.json")).unwrap(),
            before
        );
        owner.close().unwrap();
    }
    #[test]
    fn existing_live_owner_requires_authenticated_status_and_is_reused() {
        let prepared = BrokerRoot::prepare().unwrap();
        let root = prepared.root();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut owner = Instance::claim(root).unwrap();
        let endpoint = owner
            .publish(listener.local_addr().unwrap().port(), SOURCE)
            .unwrap();
        let token = endpoint.token().to_owned();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let accept_until = deadline();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock && !accept_until.expired() =>
                    {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(error) => panic!("bounded fixture accept: {error}"),
                }
            };
            let cancel = Cancellation::default();
            let request =
                broker_http::read_request(&mut stream, &token, deadline(), &cancel).unwrap();
            assert_eq!(request["operation"], "status");
            broker_http::write_response(
                &mut stream,
                200,
                &serde_json::json!({"result":{"source":SOURCE,"retiring":false}}),
                deadline(),
                &cancel,
            )
            .unwrap();
        });
        let result = ensure(
            root,
            &std::env::current_exe().unwrap(),
            vec![],
            BTreeMap::new(),
            SOURCE,
            deadline(),
            &Cancellation::default(),
        );
        server.join().unwrap();
        assert_eq!(result.unwrap().identity(), endpoint.identity());
        owner.close().unwrap();
    }
}
