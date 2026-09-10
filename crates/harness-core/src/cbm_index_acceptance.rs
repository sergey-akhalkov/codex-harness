//! Explicit real-package acceptance. All daemons, caches and source are owned.
use super::*;
use crate::{
    cancellable_pipe::{CancellablePipe, anonymous_pipe},
    mcp_protocol::{Decoder, Message, READ_CHUNK},
    process::OwnedProcess,
};
use std::path::PathBuf;

struct Peer {
    // Drop the Job before pipe workers on assertion failure.
    job: Option<Job>,
    child: OwnedProcess,
    input: Option<CancellablePipe>,
    output: CancellablePipe,
    decoder: Decoder,
    stop: Cancellation,
}

impl Peer {
    fn start(executable: &Path, cache: &Path, root: &Path) -> Self {
        let mut command = CommandSpec::new(executable);
        native::environment(&mut command, root).unwrap();
        command.current_dir = Some(root.into());
        command
            .env
            .insert("CBM_CACHE_DIR".into(), Some(cache.as_os_str().into()));
        for (key, value) in [("CBM_WORKERS", "2"), ("CBM_MEM_BUDGET_MB", "1024")] {
            command.env.insert(key.into(), Some(value.into()));
        }
        let (input, writer) = anonymous_pipe(65536).unwrap();
        let (reader, output) = anonymous_pipe(65536).unwrap();
        command.stdin = Some(input);
        command.stdout = Some(output);
        command.stderr = Some(File::create(root.join("stderr.log")).unwrap());
        let job = Job::new(Limits {
            memory_bytes: Some(2 * 1024 * 1024 * 1024),
            cpu_percent: Some(25.0),
        })
        .unwrap();
        let child = job.spawn(&command).unwrap();
        drop(command);
        let stop = Cancellation::default();
        Self {
            job: Some(job),
            child,
            input: Some(CancellablePipe::writer(writer, stop.clone()).unwrap()),
            output: CancellablePipe::reader(reader, stop.clone()).unwrap(),
            decoder: Decoder::default(),
            stop,
        }
    }

    fn send(&mut self, value: Value, deadline: Deadline) {
        let message = Message::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        self.input
            .as_mut()
            .unwrap()
            .write_all(&message.encode().unwrap(), deadline, &self.stop)
            .unwrap();
    }

    fn request(&mut self, value: Value) -> Value {
        let deadline = Deadline::after(Duration::from_secs(30)).unwrap();
        let id = value["id"].clone();
        self.send(value, deadline);
        // Bounded notifications, with one absolute read deadline.
        for _ in 0..128 {
            let message = loop {
                if let Some(message) = self.decoder.next_message().unwrap() {
                    break message.into_value();
                }
                let bytes = self.output.read(READ_CHUNK, deadline, &self.stop).unwrap();
                assert!(!bytes.is_empty(), "unexpected peer EOF");
                self.decoder.push(&bytes).unwrap();
            };
            if message.get("id").is_none() {
                continue;
            }
            assert_eq!(message["id"], id);
            assert!(message.get("error").is_none(), "{message}");
            return message["result"].clone();
        }
        panic!("peer notification limit");
    }

    fn query(&mut self, id: &str) -> Value {
        self.request(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":"query_graph","arguments":query_arguments()}}))
    }

    fn finish(mut self) {
        self.input
            .take()
            .unwrap()
            .close(Deadline::after(CLEANUP).unwrap())
            .unwrap();
        let outcome = self
            .job
            .take()
            .unwrap()
            .wait(
                &self.child,
                Deadline::after(Duration::from_secs(15)).unwrap(),
                &self.stop,
                CLEANUP,
            )
            .unwrap();
        assert_eq!(outcome.reason, StopReason::Exited, "{outcome:?}");
        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.job.active_processes, 0);
    }
}

fn query_arguments() -> Value {
    json!({"project":"native-cbm-coexist-owned",
        "query":"MATCH (n:Function) WHERE n.name = 'alpha' RETURN n.name", "format":"json"})
}

#[test]
#[ignore = "explicit audited CBM artifact; owned native daemon/cache integration"]
fn actual_tools_refuse_a_busy_cache_then_preserve_queries_and_errors() {
    let executable = PathBuf::from(
        std::env::var_os("HARNESS_CBM_EXECUTABLE").expect("explicit audited CBM required"),
    );
    let root = native::private_directory().unwrap().keep();
    // Keep logs/source for failure diagnosis; no global registrations or caches.
    println!("Owned CBM coexistence evidence: {}", root.display());
    let cache = root.join("cache");
    let runtime = root.join("runtime");
    let account = root.join("account");
    let repository = root.join("repository");
    let peer_root = root.join("peer");
    for path in [&cache, &runtime, &account, &repository, &peer_root] {
        fs::create_dir(path).unwrap();
    }
    crate::cbm_configuration::initialize_private(&cache).unwrap();
    fs::write(
        repository.join("sample.rs"),
        b"pub fn alpha() -> usize { 7 }\n",
    )
    .unwrap();
    let stop = Cancellation::default();
    let indexed = index(
        &executable,
        &cache,
        &runtime,
        &account,
        &json!({"repo_path":repository,"name":"native-cbm-coexist-owned","mode":"fast"}),
        &stop,
    )
    .unwrap();
    assert_eq!(indexed["result"]["isError"], false, "{indexed}");
    let mut peer = Peer::start(&executable, &cache, &peer_root);
    let init = peer.request(json!({"jsonrpc":"2.0","id":"init","method":"initialize",
        "params":{"protocolVersion":"2024-11-05","capabilities":{},
        "clientInfo":{"name":"owned-coexistence-check","version":"1"}}}));
    assert_eq!(init["serverInfo"]["name"], "codebase-memory-mcp");
    peer.send(
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        Deadline::after(CLEANUP).unwrap(),
    );
    let before = peer.query("before");
    assert_eq!(
        before["structuredContent"]["rows"],
        json!([["alpha"]]),
        "{before}"
    );
    // Upstream keeps a cache-wide exclusive writer on its operation log. The
    // explicit private-daemon command must refuse that conflict without
    // disconnecting, draining or adopting the separate live daemon.
    let conflict = call(
        &executable,
        &cache,
        "query_graph",
        &query_arguments(),
        &stop,
    )
    .unwrap_err();
    assert_eq!(conflict.kind(), io::ErrorKind::WouldBlock, "{conflict}");
    assert_eq!(
        conflict.to_string(),
        "CBM cache is in use by another daemon; no worker started"
    );
    assert!(peer.child.is_running().unwrap());
    let after = peer.query("after");
    assert_eq!(after, before);
    peer.finish();
    let peer_log = fs::read_to_string(peer_root.join("stderr.log")).unwrap();
    let daemon_log = fs::read_to_string(cache.join("logs/cbm-daemon.log")).unwrap();
    assert!(
        !daemon_log.contains("msg=ui.serving"),
        "owned fixture unexpectedly enabled UI"
    );
    assert!(!peer_log.contains("msg=ui.serving"));
    // Query execution is verified once the separate owner has closed normally.
    // Shared broker clients are a different, still unfinished acceptance path.
    let queried = call(
        &executable,
        &cache,
        "query_graph",
        &query_arguments(),
        &stop,
    )
    .unwrap();
    assert_eq!(queried["result"], before);
    assert_eq!(queried["owned_tree_stopped"], true);
    assert_eq!(queried["temporary_state_removed"], true);
    let coverage = call(
        &executable,
        &cache,
        "check_index_coverage",
        &json!({"project":"native-cbm-coexist-owned","paths":["sample.rs"]}),
        &stop,
    )
    .unwrap();
    assert_eq!(coverage["result"]["isError"], false);
    assert_eq!(
        coverage["result"]["structuredContent"]["paths"][0]["path"],
        "sample.rs"
    );
    let failed = call(
        &executable,
        &cache,
        "query_graph",
        &json!({"project":"native-cbm-coexist-owned","query":"MATCH ((( INVALID","format":"json"}),
        &stop,
    )
    .unwrap();
    assert_eq!(failed["result"]["isError"], true);
    assert_eq!(failed["owned_tree_stopped"], true);
    assert_eq!(failed["temporary_state_removed"], true);
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "index":indexed, "query":queried, "coverage":coverage, "tool_error":failed,
            "peer_before":before, "peer_after":after, "peer_clean_shutdown":true,
            "busy_cache_refused_before_spawn":true, "owned_ui_disabled":true
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(!stop.is_cancelled());
}
