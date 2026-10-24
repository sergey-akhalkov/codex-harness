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

const CHECKS: [&str; 6] = [
    "symbol-overview",
    "symbol-body",
    "references",
    "semantic-edit",
    "semantic-readback",
    "disk-readback",
];

const BEFORE_MARKER: &str = "7101";
const AFTER_MARKER: &str = "7102";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Language {
    Rust,
    Python,
    Cpp,
    CSharp,
    Pascal,
}

impl Language {
    const ALL: [Self; 5] = [
        Self::Rust,
        Self::Python,
        Self::Cpp,
        Self::CSharp,
        Self::Pascal,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Cpp => "cpp",
            Self::CSharp => "csharp",
            Self::Pascal => "pascal",
        }
    }

    const fn registry_id(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Cpp => "cpp",
            Self::CSharp => "csharp",
            Self::Pascal => "delphi",
        }
    }

    const fn serena_id(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python_basedpyright",
            Self::Cpp => "cpp",
            Self::CSharp => "csharp",
            Self::Pascal => "pascal",
        }
    }

    const fn required(self) -> bool {
        matches!(self, Self::Rust | Self::Python)
    }
}

#[derive(Debug)]
enum Outcome {
    Ready,
    Unavailable(String),
    Failed(String),
}

#[derive(Debug)]
struct LanguageResult {
    language: Language,
    outcome: Outcome,
    checks: Vec<&'static str>,
}

impl LanguageResult {
    fn unavailable(language: Language, detail: impl Into<String>) -> Self {
        Self {
            language,
            outcome: Outcome::Unavailable(detail.into()),
            checks: Vec::new(),
        }
    }

    fn json(&self) -> Value {
        let (status, detail) = match &self.outcome {
            Outcome::Ready => ("ready", None),
            Outcome::Unavailable(detail) => ("unavailable", Some(detail)),
            Outcome::Failed(detail) => ("failed", Some(detail)),
        };
        let mut result = json!({
            "language":self.language.name(),
            "required":self.language.required(),
            "status":status,
            "checks":self.checks,
        });
        if let Some(detail) = detail {
            result["detail"] = json!(detail);
        }
        result
    }
}

fn language_index(language: Language) -> usize {
    Language::ALL
        .iter()
        .position(|candidate| *candidate == language)
        .expect("all languages belong to the supported set")
}

fn summary(results: &[LanguageResult]) -> Value {
    let mut ordered: Vec<&LanguageResult> = results.iter().collect();
    ordered.sort_by_key(|result| language_index(result.language));
    let mandatory_ready = Language::ALL
        .iter()
        .filter(|language| language.required())
        .all(|language| {
            ordered.iter().any(|result| {
                result.language == *language && matches!(result.outcome, Outcome::Ready)
            })
        });
    let ready = ordered
        .iter()
        .filter(|result| matches!(result.outcome, Outcome::Ready))
        .map(|result| result.language.name())
        .collect::<Vec<_>>();
    let optional_failures = ordered
        .iter()
        .filter(|result| {
            !result.language.required() && matches!(result.outcome, Outcome::Failed(_))
        })
        .map(|result| result.language.name())
        .collect::<Vec<_>>();
    let optional_unavailable = ordered
        .iter()
        .filter(|result| {
            !result.language.required() && matches!(result.outcome, Outcome::Unavailable(_))
        })
        .map(|result| result.language.name())
        .collect::<Vec<_>>();
    json!({
        "status":if mandatory_ready {"semantic-ready"} else {"semantic-not-ready"},
        "languages":ready,
        "required_languages":Language::ALL.iter()
            .filter(|language| language.required())
            .map(|language| language.name())
            .collect::<Vec<_>>(),
        "checks":CHECKS,
        "results":ordered.iter().map(|result| result.json()).collect::<Vec<_>>(),
        "optional_failures":optional_failures,
        "optional_unavailable":optional_unavailable,
    })
}

struct Prepared {
    registry: Value,
    selected: Vec<Language>,
    unavailable: Vec<LanguageResult>,
}

fn preflight(inventory: &Value) -> Prepared {
    let rows = inventory["languages"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut valid = Vec::new();
    let mut selected = Vec::new();
    let mut unavailable = Vec::new();
    for language in Language::ALL {
        let matching = rows
            .iter()
            .filter(|row| row["id"] == language.registry_id())
            .collect::<Vec<_>>();
        if matching.is_empty() {
            unavailable.push(LanguageResult::unavailable(language, "no registry record"));
            continue;
        }
        if matching.len() > 1 {
            unavailable.push(LanguageResult::unavailable(
                language,
                format!("ambiguous: {} registry records", matching.len()),
            ));
            continue;
        }
        let row = matching[0];
        let status = row["status"].as_str().unwrap_or("missing");
        let probe = json!({"languages":[row]});
        match crate::serena_configuration::ls_specific_settings(&probe) {
            Ok(settings) if settings.contains_key(language.serena_id()) => {
                valid.push(row.clone());
                selected.push(language);
            }
            Ok(_) => unavailable.push(LanguageResult::unavailable(
                language,
                format!("registry status '{status}' has no adopted launch setting"),
            )),
            Err(error) => {
                unavailable.push(LanguageResult::unavailable(language, error.to_string()));
            }
        }
    }
    let mut registry = inventory.clone();
    if !registry.is_object() {
        registry = json!({});
    }
    registry["languages"] = Value::Array(valid);
    Prepared {
        registry,
        selected,
        unavailable,
    }
}

struct Fixture {
    relative: &'static str,
    caller: &'static str,
    symbol: &'static str,
    reference_name: &'static str,
    edit_name: &'static str,
    caller_marker: &'static str,
    after: &'static str,
}

fn prepare_project(state: &Path, language: Language) -> io::Result<(std::path::PathBuf, Fixture)> {
    let project = state.join(language.name());
    fs::create_dir_all(project.join(".serena"))?;
    fs::write(
        project.join(".serena/project.yml"),
        format!(
            "project_name: acceptance-{}\nlanguage_servers:\n- {}\nencoding: {}\n",
            language.name(),
            language.serena_id(),
            if language == Language::Pascal {
                "cp1251"
            } else {
                "utf-8"
            }
        ),
    )?;
    let fixture = match language {
        Language::Rust => {
            fs::create_dir_all(project.join("src"))?;
            fs::write(
                project.join("Cargo.toml"),
                "[package]\nname = \"semantic-acceptance\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            )?;
            fs::write(
                project.join("src/lib.rs"),
                "pub mod consumer;\npub fn acceptance_value() -> i32 { 7101 }\n",
            )?;
            fs::write(
                project.join("src/consumer.rs"),
                "pub fn acceptance_caller() -> i32 { crate::acceptance_value() }\n",
            )?;
            Fixture {
                relative: "src/lib.rs",
                caller: "src/consumer.rs",
                symbol: "acceptance_value",
                reference_name: "acceptance_value",
                edit_name: "acceptance_value",
                caller_marker: "consumer.rs",
                after: "pub fn acceptance_value() -> i32 { 7102 }",
            }
        }
        Language::Python => {
            fs::write(
                project.join("sample.py"),
                "def acceptance_value() -> int:\n    return 7101\n",
            )?;
            fs::write(
                project.join("caller.py"),
                "from sample import acceptance_value\n\ndef acceptance_caller() -> int:\n    return acceptance_value()\n",
            )?;
            Fixture {
                relative: "sample.py",
                caller: "caller.py",
                symbol: "acceptance_value",
                reference_name: "acceptance_value",
                edit_name: "acceptance_value",
                caller_marker: "caller.py",
                after: "def acceptance_value() -> int:\n    return 7102",
            }
        }
        Language::Cpp => {
            fs::write(project.join("compile_flags.txt"), "-std=c++17\n")?;
            fs::write(
                project.join("CMakeLists.txt"),
                "# owned acceptance fixture\n",
            )?;
            fs::write(project.join("value.h"), "int acceptance_value();\n")?;
            fs::write(
                project.join("caller.cpp"),
                "#include \"value.h\"\nint acceptance_caller() { return acceptance_value(); }\n",
            )?;
            fs::write(
                project.join("value.cpp"),
                "#include \"value.h\"\nint acceptance_value() { return 7101; }\n",
            )?;
            Fixture {
                relative: "value.cpp",
                caller: "caller.cpp",
                symbol: "acceptance_value",
                reference_name: "acceptance_value",
                edit_name: "acceptance_value",
                caller_marker: "caller.cpp",
                after: "int acceptance_value() { return 7102; }",
            }
        }
        Language::CSharp => {
            fs::write(
                project.join("Acceptance.csproj"),
                "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <TargetFramework>net10.0</TargetFramework>\n  </PropertyGroup>\n</Project>\n",
            )?;
            fs::write(
                project.join("Value.cs"),
                "namespace Acceptance;\npublic static class Value\n{\n    public static int AcceptanceValue() => 7101;\n}\n",
            )?;
            fs::write(
                project.join("Caller.cs"),
                "namespace Acceptance;\npublic static class Caller\n{\n    public static int Call() => Value.AcceptanceValue();\n}\n",
            )?;
            Fixture {
                relative: "Value.cs",
                caller: "Caller.cs",
                symbol: "AcceptanceValue",
                reference_name: "AcceptanceValue",
                edit_name: "AcceptanceValue",
                caller_marker: "Caller.cs",
                after: "public static int AcceptanceValue() => 7102;",
            }
        }
        Language::Pascal => {
            fs::write(
                project.join("acceptance_unit.pas"),
                b"unit acceptance_unit;{ \xef\xf0\xe8\xe2\xe5\xf2, test CP1251 }\r\ninterface\r\nimplementation\r\nfunction acceptance_value: Integer;\r\nbegin\r\n  acceptance_value := 7101;\r\nend;\r\nprocedure acceptance_caller;\r\nbegin\r\n  acceptance_value;\r\nend;\r\nend.\r\n",
            )?;
            Fixture {
                relative: "acceptance_unit.pas",
                caller: "acceptance_unit.pas",
                symbol: "acceptance_value",
                reference_name: "acceptance_value",
                edit_name: "acceptance_value",
                caller_marker: "acceptance_caller",
                after: "function acceptance_value: Integer;\r\nbegin\r\n  acceptance_value := 7102;\r\nend;",
            }
        }
    };
    Ok((project, fixture))
}

fn at(stage: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{stage}: {error}"))
}

struct Exercise<'a> {
    source: &'a Path,
    registry: &'a Path,
    home: &'a Path,
    state: &'a Path,
    console: &'a Path,
    arguments: &'a [OsString],
    initialize: &'a Value,
    deadline: Deadline,
    cancel: &'a Cancellation,
}

impl Exercise<'_> {
    fn run(&self, language: Language, clients: &mut Vec<Client>) -> (Vec<&'static str>, Outcome) {
        let mut checks = Vec::new();
        let result = (|| -> io::Result<()> {
            let (project, fixture) =
                prepare_project(self.state, language).map_err(|error| at("fixture", error))?;
            let client = Client::new(
                Configuration {
                    serena: self.console.into(),
                    registry: self.registry.into(),
                    codex_home: self.home.to_owned(),
                    source_root: self.source.into(),
                },
                self.deadline,
                self.cancel,
            )
            .map_err(|error| at("broker", error))?;
            clients.push(client);
            let client = clients
                .last()
                .ok_or_else(|| io::Error::other("Serena acceptance client was lost"))?;
            let connected = client
                .connect(
                    self.arguments,
                    &project,
                    self.initialize,
                    self.deadline,
                    self.cancel,
                )
                .map_err(|error| at("connect", error))?;
            require(
                connected["message"]["result"]["serverInfo"]["name"] == "Serena",
                "Serena acceptance handshake failed",
            )
            .map_err(|error| at("connect", error))?;
            let route = &connected["route"];
            let overview = call(
                client,
                route,
                self.initialize,
                "get_symbols_overview",
                json!({"relative_path":fixture.relative,"depth":1,"max_answer_chars":3800}),
                self.deadline,
                self.cancel,
            )
            .map_err(|error| at("symbol overview", error))?;
            require(
                overview.contains(fixture.symbol),
                "symbol overview missed the fixture symbol",
            )
            .map_err(|error| at("symbol overview", error))?;
            checks.push(CHECKS[0]);
            let query = json!({
                "relative_path":fixture.relative,
                "name_path_pattern":fixture.symbol,
                "include_body":true,
                "max_answer_chars":3800,
            });
            let found = call(
                client,
                route,
                self.initialize,
                "find_symbol",
                query.clone(),
                self.deadline,
                self.cancel,
            )
            .map_err(|error| at("symbol body", error))?;
            require(
                found.contains(BEFORE_MARKER),
                "symbol body is stale or missing",
            )
            .map_err(|error| at("symbol body", error))?;
            checks.push(CHECKS[1]);
            call(
                client,
                route,
                self.initialize,
                "get_symbols_overview",
                json!({"relative_path":fixture.caller,"max_answer_chars":2000}),
                self.deadline,
                self.cancel,
            )
            .map_err(|error| at("caller overview", error))?;
            let references = call(
                client,
                route,
                self.initialize,
                "find_referencing_symbols",
                json!({
                    "relative_path":fixture.relative,
                    "name_path":fixture.reference_name,
                    "max_answer_chars":3800,
                }),
                self.deadline,
                self.cancel,
            )
            .map_err(|error| at("references", error))?;
            require(
                references.contains(fixture.caller_marker),
                "cross-file reference missing",
            )
            .map_err(|error| at("references", error))?;
            checks.push(CHECKS[2]);
            call(
                client,
                route,
                self.initialize,
                "replace_symbol_body",
                json!({
                    "relative_path":fixture.relative,
                    "name_path":fixture.edit_name,
                    "body":fixture.after,
                }),
                self.deadline,
                self.cancel,
            )
            .map_err(|error| at("semantic edit", error))?;
            checks.push(CHECKS[3]);
            let changed = call(
                client,
                route,
                self.initialize,
                "find_symbol",
                query,
                self.deadline,
                self.cancel,
            )
            .map_err(|error| at("semantic readback", error))?;
            require(
                changed.contains(AFTER_MARKER) && !changed.contains(BEFORE_MARKER),
                "semantic readback did not observe the edit",
            )
            .map_err(|error| at("semantic readback", error))?;
            checks.push(CHECKS[4]);
            let actual = fs::read(project.join(fixture.relative))
                .map_err(|error| at("disk readback", error))?;
            require(
                actual
                    .windows(AFTER_MARKER.len())
                    .any(|window| window == AFTER_MARKER.as_bytes())
                    && !actual
                        .windows(BEFORE_MARKER.len())
                        .any(|window| window == BEFORE_MARKER.as_bytes()),
                "semantic edit was not persisted to the owned fixture",
            )
            .map_err(|error| at("disk readback", error))?;
            checks.push(CHECKS[5]);
            Ok(())
        })();
        let outcome = match result {
            Ok(()) => Outcome::Ready,
            Err(error) => Outcome::Failed(error.to_string()),
        };
        (checks, outcome)
    }
}

/// One run checks every supported adopted backend. Rust and Python are
/// mandatory; an unavailable or broken optional backend is reported without
/// erasing successful mandatory results or provisioning anything.
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
    let prepared = preflight(&inventory);
    let state = tempfile::Builder::new()
        .prefix("harness-serena-check-")
        .tempdir()?;
    let home = state.path().join("codex-home");
    let filtered_registry = state.path().join("acceptance-registry.json");
    let deadline = Deadline::after(Duration::from_secs(600))?;
    let cancel = Cancellation::default();
    let initialize = json!({"protocolVersion":"2024-11-05","capabilities":{},
        "clientInfo":{"name":"harness-semantic-acceptance","version":"1"}});
    let mut clients = Vec::new();
    let result: io::Result<Value> = (|| {
        fs::write(&filtered_registry, serde_json::to_vec(&prepared.registry)?)?;
        let exercise = Exercise {
            source,
            registry: &filtered_registry,
            home: &home,
            state: state.path(),
            console: Path::new(console),
            arguments: &arguments,
            initialize: &initialize,
            deadline,
            cancel: &cancel,
        };
        let mut results = prepared.unavailable;
        for language in prepared.selected {
            let (checks, outcome) = exercise.run(language, &mut clients);
            results.push(LanguageResult {
                language,
                outcome,
                checks,
            });
        }
        let mut report = summary(&results);
        report["manager_sha256"] =
            json!(crate::build_identity::hash_file(&std::env::current_exe()?)?);
        Ok(report)
    })();
    let cleanup = (|| {
        let mut failures = Vec::new();
        for client in &clients {
            if let Err(error) = client.disconnect() {
                failures.push(format!("disconnect: {error}"));
            }
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
            if matches!(retired, broker_launch::Retirement::Pending { .. }) {
                failures.push("broker retirement pending".to_owned());
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(io::Error::other(failures.join("; ")))
        }
    })();
    match (result, cleanup) {
        (Ok(mut report), Ok(())) if report["status"] == "semantic-ready" => {
            state.close()?;
            report["owned_state_removed"] = json!(true);
            Ok(report)
        }
        (result, cleanup) => {
            let evidence = state.keep();
            let result = match result {
                Ok(report) if report["status"] == "semantic-not-ready" => {
                    format!("not ready: {report}")
                }
                Ok(report) => format!("checks passed: {report}"),
                Err(error) => error.to_string(),
            };
            Err(io::Error::other(format!(
                "Serena semantic acceptance failed: {result}; cleanup: {}; evidence: {}",
                cleanup
                    .err()
                    .map_or_else(|| "complete".into(), |error| error.to_string()),
                evidence.display()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &Path) -> String {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"inert").unwrap();
        path.to_string_lossy().into_owned()
    }

    fn directory(path: &Path) -> String {
        fs::create_dir_all(path).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn row(language: Language, root: &Path) -> Value {
        let mut paths = json!({});
        match language {
            Language::Rust | Language::Cpp => {
                paths["executable"] = json!(file(
                    &root.join(format!("{}-backend.exe", language.registry_id()))
                ));
            }
            Language::Python => {
                paths["node"] = json!(file(&root.join("node.exe")));
                paths["entrypoint"] = json!(file(&root.join("basedpyright/langserver.index.js")));
            }
            Language::CSharp => {
                paths["dotnet"] = json!(file(&root.join("dotnet.exe")));
                paths["entrypoint"] = json!(file(&root.join("roslyn/LanguageServer.dll")));
                paths["runtime_root"] = json!(directory(&root.join("roslyn")));
            }
            Language::Pascal => {
                paths["executable"] = json!(file(&root.join("pasls/pasls.exe")));
                paths["fpc"] = json!(file(&root.join("fpc/fpc.exe")));
                paths["fpc_source"] = json!(directory(&root.join("fpc/source")));
            }
        }
        let mut record = json!({
            "id":language.registry_id(),
            "serena_id":language.serena_id(),
            "status":"adopted",
            "paths":paths,
        });
        if language == Language::CSharp {
            record["version"] = json!("5.11.0");
        }
        record
    }

    fn inventory(rows: Vec<Value>) -> Value {
        json!({"mcp":[],"languages":rows})
    }

    fn ready(language: Language) -> LanguageResult {
        LanguageResult {
            language,
            outcome: Outcome::Ready,
            checks: CHECKS.to_vec(),
        }
    }

    fn result(language: Language, outcome: Outcome) -> LanguageResult {
        LanguageResult {
            language,
            outcome,
            checks: Vec::new(),
        }
    }

    fn detail(prepared: &Prepared, language: Language) -> String {
        let result = prepared
            .unavailable
            .iter()
            .find(|result| result.language == language)
            .expect("explicit language result");
        match &result.outcome {
            Outcome::Unavailable(detail) | Outcome::Failed(detail) => detail.clone(),
            Outcome::Ready => panic!("unexpected ready result"),
        }
    }

    #[test]
    fn all_supported_adopted_backends_are_selected_for_one_run() {
        let root = tempfile::tempdir().unwrap();
        let prepared = preflight(&inventory(
            Language::ALL
                .iter()
                .map(|language| row(*language, root.path()))
                .collect(),
        ));
        assert_eq!(prepared.selected, Language::ALL);
        assert!(prepared.unavailable.is_empty());
        assert_eq!(
            prepared.registry["languages"].as_array().unwrap().len(),
            Language::ALL.len()
        );
    }

    #[test]
    fn optional_prerequisite_failures_are_explicit_and_do_not_erase_mandatory_work() {
        let root = tempfile::tempdir().unwrap();
        let mut csharp = row(Language::CSharp, root.path());
        csharp["paths"]["entrypoint"] = json!(root.path().join("roslyn/gone/LanguageServer.dll"));
        let mut pascal = row(Language::Pascal, root.path());
        pascal["status"] = json!("broken");
        let prepared = preflight(&inventory(vec![
            row(Language::Rust, root.path()),
            row(Language::Python, root.path()),
            row(Language::Cpp, root.path()),
            row(Language::Cpp, root.path()),
            csharp,
            pascal,
        ]));
        assert_eq!(prepared.selected, vec![Language::Rust, Language::Python]);
        assert!(detail(&prepared, Language::Cpp).contains("ambiguous"));
        assert!(detail(&prepared, Language::CSharp).contains("Roslyn"));
        assert_eq!(
            detail(&prepared, Language::Pascal),
            "registry status 'broken' has no adopted launch setting"
        );
    }

    #[test]
    fn report_contains_every_language_and_never_upgrades_mandatory_failure() {
        let results = vec![
            ready(Language::Rust),
            result(
                Language::Python,
                Outcome::Failed("connect: backend exited with code 1".to_owned()),
            ),
            result(
                Language::Cpp,
                Outcome::Failed("references: backend timeout".to_owned()),
            ),
            LanguageResult::unavailable(Language::CSharp, "registry status 'missing'"),
            ready(Language::Pascal),
        ];
        let report = summary(&results);
        assert_eq!(report["status"], "semantic-not-ready");
        assert_eq!(report["languages"], json!(["rust", "pascal"]));
        assert_eq!(report["optional_failures"], json!(["cpp"]));
        assert_eq!(
            report["checks"],
            json!([
                "symbol-overview",
                "symbol-body",
                "references",
                "semantic-edit",
                "semantic-readback",
                "disk-readback"
            ])
        );
        let result_rows = report["results"].as_array().unwrap();
        assert_eq!(result_rows.len(), Language::ALL.len());
        let python = result_rows
            .iter()
            .find(|result| result["language"] == "python")
            .unwrap();
        assert_eq!(python["status"], "failed");
        assert_eq!(python["required"], true);
        assert!(
            python["detail"]
                .as_str()
                .unwrap()
                .contains("backend exited")
        );
        let csharp = result_rows
            .iter()
            .find(|result| result["language"] == "csharp")
            .unwrap();
        assert_eq!(csharp["status"], "unavailable");
        assert_eq!(csharp["required"], false);
        // An optional failure remains visible but mandatory readiness is not
        // downgraded by it.
        let ready_mandatory = vec![
            ready(Language::Rust),
            ready(Language::Python),
            result(
                Language::Cpp,
                Outcome::Failed("semantic edit: backend rejected edit".to_owned()),
            ),
            LanguageResult::unavailable(Language::CSharp, "no registry record"),
            LanguageResult::unavailable(Language::Pascal, "registry status 'broken'"),
        ];
        let report = summary(&ready_mandatory);
        assert_eq!(report["status"], "semantic-ready");
        assert_eq!(report["optional_failures"], json!(["cpp"]));
        assert_eq!(report["languages"], json!(["rust", "python"]));
    }
}
