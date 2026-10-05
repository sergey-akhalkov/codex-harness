//! Semantic acceptance of the adopted Serena through the native shared broker.
//! Fixtures and brokers belong to this invocation; user projects are never edited.
#![cfg(windows)]

use crate::{
    broker_launch,
    broker_state::BrokerRoot,
    process::{Cancellation, Deadline},
    serena_broker::{Client, Configuration},
};
use serde_json::{Value, json};
use std::{ffi::OsString, fs, io, path::Path, time::Duration};

fn require(condition: bool, detail: &str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(detail.to_owned()))
    }
}

fn content(response: &Value) -> io::Result<String> {
    let message = &response["message"];
    require(
        message.get("error").is_none() && message["result"]["isError"] != true,
        &format!("Serena semantic acceptance failed: {message}"),
    )?;
    let blocks = message["result"]["content"]
        .as_array()
        .ok_or_else(|| io::Error::other("Serena response has no content"))?;
    Ok(blocks
        .iter()
        .filter_map(|block| block["text"].as_str())
        .collect())
}

fn call(
    client: &Client,
    route: &Value,
    initialize: &Value,
    name: &str,
    arguments: Value,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<String> {
    content(&client.rpc(
        "tools/call",
        &json!({"name":name,"arguments":arguments}),
        route,
        initialize,
        deadline,
        cancel,
    )?)
}

/// No provisioning and no user-state changes. Missing adopted dependencies fail.
pub fn verify(source: &Path, registry: &Path) -> io::Result<Value> {
    let inventory: Value = serde_json::from_slice(&fs::read(registry)?)?;
    let console = inventory["mcp"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"] == "serena"))
        .and_then(|item| item["paths"]["console_entrypoint"].as_str())
        .ok_or_else(|| {
            io::Error::other("Serena acceptance requires an adopted console entrypoint")
        })?;
    let catalogue: Value =
        serde_json::from_slice(&fs::read(source.join("global/code-tools.json"))?)?;
    let arguments = catalogue["mcp"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"] == "serena"))
        .and_then(|item| item["arguments"].as_array())
        .ok_or_else(|| io::Error::other("Serena launch arguments are missing"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(OsString::from)
                .ok_or_else(|| io::Error::other("invalid Serena launch argument"))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let state = tempfile::Builder::new()
        .prefix("harness-serena-check-")
        .tempdir()?;
    let home = state.path().join("codex-home");
    let deadline = Deadline::after(Duration::from_secs(240))?;
    let cancel = Cancellation::default();
    let initialize = json!({"protocolVersion":"2024-11-05","capabilities":{},
        "clientInfo":{"name":"harness-semantic-acceptance","version":"1"}});
    let mut clients = Vec::new();
    let result: io::Result<Value> = (|| {
        // Pascal navigation is retained only where discovery adopted the
        // verified shared pasls installation; absence keeps the acceptance on
        // the remaining backends instead of provisioning anything.
        let pascal_adopted = inventory["languages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|row| row["id"] == "delphi" && row["status"] == "adopted");
        let mut languages: Vec<&str> = vec!["rust", "python"];
        if pascal_adopted {
            languages.push("pascal");
        }
        let mut exercised: Vec<&str> = Vec::new();
        for language in languages {
            let project = state.path().join(language);
            fs::create_dir_all(project.join(".serena"))?;
            let encoding = if language == "pascal" { "cp1251" } else { "utf-8" };
            fs::write(
                project.join(".serena/project.yml"),
                format!(
                    "project_name: acceptance-{language}\nlanguage_servers:\n- {language}\nencoding: {encoding}\n"
                ),
            )?;
            let (relative, before, after): (&str, Vec<u8>, String) = if language == "rust" {
                fs::create_dir_all(project.join("src"))?;
                fs::write(
                    project.join("Cargo.toml"),
                    "[package]\nname = \"semantic-acceptance\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                )?;
                (
                    "src/lib.rs",
                    b"pub fn acceptance_value() -> i32 { 7101 }\n".to_vec(),
                    "pub fn acceptance_value() -> i32 { 7102 }".to_owned(),
                )
            } else if language == "pascal" {
                (
                    "unit1.pas",
                    b"unit acceptance_unit;{ \xef\xf0\xe8\xe2\xe5\xf2 }\r\ninterface\r\nimplementation\r\nfunction acceptance_value: Integer;\r\nbegin\r\n  acceptance_value := 7101;\r\nend;\r\nend.\r\n".to_vec(),
                    "function acceptance_value: Integer;\r\nbegin\r\n  acceptance_value := 7102;\r\nend;".to_owned(),
                )
            } else {
                (
                    "sample.py",
                    b"def acceptance_value() -> int:\n    return 7101\n".to_vec(),
                    "def acceptance_value() -> int:\n    return 7102".to_owned(),
                )
            };
            fs::write(project.join(relative), &before)?;
            clients.push(Client::new(
                Configuration {
                    serena: console.into(),
                    registry: registry.into(),
                    codex_home: home.clone(),
                    source_root: source.into(),
                },
                deadline,
                &cancel,
            )?);
            let client = clients.last().expect("just inserted");
            let connected = client.connect(&arguments, &project, &initialize, deadline, &cancel)?;
            require(
                connected["message"]["result"]["serverInfo"]["name"] == "Serena",
                "Serena acceptance handshake failed",
            )?;
            let route = &connected["route"];
            let overview = call(
                client,
                route,
                &initialize,
                "get_symbols_overview",
                // One level of descendants so unit symbols grouped under a
                // backend section pseudo-namespace (legacy Pascal) appear.
                json!({"relative_path":relative,"depth":1,"max_answer_chars":3800}),
                deadline,
                &cancel,
            )?;
            require(
                overview.contains("acceptance_value"),
                "Serena overview missed the fixture symbol",
            )?;
            let query = json!({"relative_path":relative,"name_path_pattern":"acceptance_value",
                "include_body":true,"max_answer_chars":3800});
            let found = call(
                client,
                route,
                &initialize,
                "find_symbol",
                query.clone(),
                deadline,
                &cancel,
            )?;
            require(
                found.contains("7101"),
                "Serena symbol body is stale or missing",
            )?;
            call(
                client,
                route,
                &initialize,
                "replace_symbol_body",
                json!({"relative_path":relative,"name_path":"acceptance_value","body":after}),
                deadline,
                &cancel,
            )?;
            let changed = call(
                client,
                route,
                &initialize,
                "find_symbol",
                query,
                deadline,
                &cancel,
            )?;
            require(
                changed.contains("7102") && !changed.contains("7101"),
                "Serena edit was not observed by semantic readback",
            )?;
            let actual = fs::read(project.join(relative))?;
            require(
                actual.windows(4).any(|window| window == b"7102".as_slice())
                    && !actual.windows(4).any(|window| window == b"7101".as_slice()),
                "Serena edit was not persisted to the owned fixture",
            )?;
            exercised.push(language);
        }
        Ok(
            json!({"status":"semantic-ready","languages":exercised,
            "checks":["symbol-overview","symbol-body","semantic-edit","semantic-readback","disk-readback"],
            "manager_sha256":crate::build_identity::hash_file(&std::env::current_exe()?)?}),
        )
    })();
    let cleanup = (|| {
        for client in &clients {
            client.disconnect()?;
        }
        let anchor = home.join("harness/runtime/serena-broker.json");
        if anchor.exists() {
            let record: Value = serde_json::from_slice(&fs::read(anchor)?)?;
            let root =
                BrokerRoot::open(Path::new(record["root"].as_str().ok_or_else(|| {
                    io::Error::other("Serena acceptance broker root missing")
                })?))?;
            let retired = broker_launch::retire(
                &root,
                Deadline::after(Duration::from_secs(60))?,
                &Cancellation::default(),
            )?;
            require(
                !matches!(retired, broker_launch::Retirement::Pending { .. }),
                "Serena acceptance broker cleanup pending",
            )?;
        }
        Ok::<_, io::Error>(())
    })();
    match (result, cleanup) {
        (Ok(mut report), Ok(())) => {
            state.close()?;
            report["owned_state_removed"] = json!(true);
            Ok(report)
        }
        (result, cleanup) => {
            let evidence = state.keep();
            Err(io::Error::other(format!(
                "Serena semantic acceptance failed: {}; cleanup: {}; evidence: {}",
                result
                    .err()
                    .map_or_else(|| "checks passed".into(), |error| error.to_string()),
                cleanup
                    .err()
                    .map_or_else(|| "complete".into(), |error| error.to_string()),
                evidence.display()
            )))
        }
    }
}
