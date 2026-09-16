//! Qualification of installed operations for improve-installed-tool-workflows.
//! Reuses the native consumer and private evidence lifecycle; no model requests.
use super::{Consumer, input};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git.exe")
        .args([
            "-c",
            "user.name=Workflow Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.hooksPath=.git/owned-empty-hooks",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}

#[test]
fn memory_text_survives_clone_and_conflicting_worktrees() {
    let run = tempfile::tempdir().unwrap();
    let project = run.path().join("project");
    fs::create_dir_all(project.join("docs/memory")).unwrap();
    fs::create_dir_all(project.join(".serena/memories")).unwrap();
    fs::create_dir_all(project.join(".serena/cache")).unwrap();
    assert!(git(&project, &["init", "--quiet"]).status.success());
    fs::write(project.join(".gitignore"), "/.serena/cache/\n").unwrap();
    fs::write(
        project.join("AGENTS.md"),
        "Read [memory](docs/memory/README.md).\n",
    )
    .unwrap();
    fs::write(
        project.join("docs/memory/README.md"),
        "[Decision](../decision.md) is authoritative.\n",
    )
    .unwrap();
    fs::write(
        project.join("docs/decision.md"),
        "Confirmed unit price: 7.\n",
    )
    .unwrap();
    fs::write(
        project.join(".serena/memories/route.md"),
        "Read docs/decision.md; update only that owner.\n",
    )
    .unwrap();
    fs::write(
        project.join(".serena/cache/runtime.bin"),
        "private runtime fixture",
    )
    .unwrap();
    assert!(
        git(&project, &["check-ignore", ".serena/cache/runtime.bin"])
            .status
            .success()
    );
    assert!(
        !git(
            &project,
            &["ls-files", "--error-unmatch", ".serena/memories/route.md"]
        )
        .status
        .success()
    );
    assert!(git(&project, &["add", "."]).status.success());
    assert!(
        git(
            &project,
            &["commit", "--quiet", "-m", "Owned portable memory fixture"]
        )
        .status
        .success()
    );
    let tracked = String::from_utf8(git(&project, &["ls-files"]).stdout).unwrap();
    assert!(tracked.contains(".serena/memories/route.md"));
    assert!(!tracked.contains("runtime.bin"));
    let clone = run.path().join("clone");
    assert!(
        git(
            run.path(),
            &[
                "clone",
                "--quiet",
                "--no-local",
                project.to_str().unwrap(),
                clone.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    // Native text reads follow the delivered entry route with no MCP or history.
    assert!(
        fs::read_to_string(clone.join("AGENTS.md"))
            .unwrap()
            .contains("docs/memory/README.md")
    );
    assert!(
        fs::read_to_string(clone.join("docs/memory/README.md"))
            .unwrap()
            .contains("../decision.md")
    );
    assert_eq!(
        fs::read_to_string(clone.join("docs/decision.md")).unwrap(),
        "Confirmed unit price: 7.\n"
    );
    assert!(!clone.join(".serena/cache/runtime.bin").exists());
    assert_eq!(
        fs::read(clone.join(".serena/memories/route.md")).unwrap(),
        fs::read(project.join(".serena/memories/route.md")).unwrap()
    );
    let branch = run.path().join("candidate");
    assert!(
        git(
            &project,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "candidate",
                branch.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    fs::write(
        branch.join("docs/decision.md"),
        "Candidate observation: unit price 8; approval pending.\n",
    )
    .unwrap();
    assert!(
        git(
            &branch,
            &["commit", "--quiet", "-am", "Owned candidate observation"]
        )
        .status
        .success()
    );
    fs::write(
        project.join("docs/decision.md"),
        "Current user decision: unit price 9.\n",
    )
    .unwrap();
    assert!(
        git(
            &project,
            &["commit", "--quiet", "-am", "Owned current decision"]
        )
        .status
        .success()
    );
    fs::write(
        project.join("unrelated.txt"),
        "Preserve this uncommitted work.\n",
    )
    .unwrap();
    assert!(
        !git(&project, &["merge", "--no-edit", "candidate"])
            .status
            .success()
    );
    assert!(
        String::from_utf8(git(&project, &["show", ":2:docs/decision.md"]).stdout)
            .unwrap()
            .contains("unit price 9")
    );
    assert!(
        String::from_utf8(git(&project, &["show", ":3:docs/decision.md"]).stdout)
            .unwrap()
            .contains("unit price 8")
    );
    // Resolve using the explicit current decision, preserving candidate evidence
    // in its fixture commit and the single authoritative record in the result.
    fs::write(
        project.join("docs/decision.md"),
        "Current user decision: unit price 9.\n",
    )
    .unwrap();
    assert!(git(&project, &["add", "docs/decision.md"]).status.success());
    assert!(
        git(
            &project,
            &[
                "commit",
                "--quiet",
                "-m",
                "Resolve owned fixture from current decision"
            ]
        )
        .status
        .success()
    );
    assert!(
        String::from_utf8(git(&project, &["show", "candidate:docs/decision.md"]).stdout)
            .unwrap()
            .contains("unit price 8")
    );
    assert_eq!(
        fs::read_to_string(project.join("unrelated.txt")).unwrap(),
        "Preserve this uncommitted work.\n"
    );
    assert!(
        git(&project, &["worktree", "remove", branch.to_str().unwrap()])
            .status
            .success()
    );
}

fn cargo_check(project: &Path, evidence: &Path, label: &str) -> bool {
    let result = Command::new("cargo.exe")
        .args(["test", "--offline", "--quiet", "--jobs", "1"])
        .current_dir(project)
        .output()
        .unwrap();
    fs::write(evidence.join(format!("{label}-stdout.txt")), &result.stdout).unwrap();
    fs::write(evidence.join(format!("{label}-stderr.txt")), &result.stderr).unwrap();
    result.status.success()
}

fn failed_tool(client: &mut Consumer, name: &str, arguments: Value) -> Value {
    let result = client.request(
        "mcpServer/tool/call",
        json!({"threadId":client.thread,"server":"serena","tool":name,"arguments":arguments}),
        90,
    );
    assert_eq!(
        result["isError"], true,
        "expected rejected {name}: {result}"
    );
    result
}

#[test]
#[ignore = "requires installed native Codex and global MCP; creates owned external fixtures, no model calls"]
fn installed_code_and_memory_operations() {
    let executable = input("CODEGRAPH_CONSUMER_CODEX_EXE");
    let home = input("CODEGRAPH_CONSUMER_HOME");
    let output = input("CODEGRAPH_CONSUMER_OUTPUT");
    let run = tempfile::Builder::new()
        .prefix("tool-workflows-")
        .tempdir_in(output)
        .unwrap()
        .keep();
    println!("Private tool-workflow evidence: {}", run.display());
    let project = run.join("project");
    for directory in ["src", "tests", "docs/memory", ".serena/memories"] {
        fs::create_dir_all(project.join(directory)).unwrap();
    }
    assert!(
        Command::new("git.exe")
            .args(["init", "--quiet"])
            .arg(&project)
            .status()
            .unwrap()
            .success()
    );
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname=\"workflow-fixture\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(
        project.join(".gitignore"),
        "/target/\n/.serena/*\n!/.serena/project.yml\n!/.serena/memories/\n",
    )
    .unwrap();
    fs::write(project.join(".serena/project.yml"), "project_name: workflow-fixture\nlanguages: [rust]\nread_only_memory_patterns: ['^protected$']\nignored_memory_patterns: ['^ignored$']\n").unwrap();
    let original = "pub fn unit_price() -> u32 { 7 }\npub fn total(quantity: u32) -> u32 { quantity * unit_price() }\n";
    fs::write(project.join("src/lib.rs"), original).unwrap();
    fs::write(project.join("src/main.rs"), "fn main() { let quantity: u32 = std::env::args().nth(1).unwrap().parse().unwrap(); println!(\"{}\", workflow_fixture::total(quantity)); }\n").unwrap();
    fs::write(project.join("tests/cli.rs"), "#[test]\nfn quantity_three_costs_twenty_one() { let output = std::process::Command::new(env!(\"CARGO_BIN_EXE_workflow-fixture\")).arg(\"3\").output().unwrap(); assert!(output.status.success()); assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), \"21\"); }\n").unwrap();
    fs::write(
        project.join("AGENTS.md"),
        "Read [project memory](docs/memory/README.md) for relevant durable decisions.\n",
    )
    .unwrap();
    fs::write(
        project.join("docs/memory/README.md"),
        "# Project memory\n\n[Pricing](../pricing.md) owns the price decision and native check.\n",
    )
    .unwrap();
    fs::write(
        project.join("docs/pricing.md"),
        "# Pricing\n\nConfirmed: unit price is 7. Verify with cargo test --offline.\n",
    )
    .unwrap();
    let memory = project.join(".serena/memories");
    fs::write(memory.join("protected.md"), "Protected fixture decision.\n").unwrap();
    fs::write(memory.join("ignored.md"), "Ignored fixture data.\n").unwrap();
    assert!(cargo_check(&project, &run, "baseline"));

    let mut client = Consumer::start(&executable, &home, &project, &run.join("client"));
    client.open_thread(&project);
    let inventory = client.request(
        "mcpServerStatus/list",
        json!({"threadId":client.thread,"limit":100,"detail":"toolsAndAuthOnly"}),
        90,
    );
    let servers = inventory["data"].as_array().unwrap();
    // This scenario exercises Serena. Graphify is retired from the managed selection.
    // CodeGraph qualification is independent and must not suppress these checks.
    for name in ["serena"] {
        assert!(servers.iter().any(|s| s["name"] == name && s["tools"].as_object().is_some_and(|t| !t.is_empty())), "missing {name}; inspect retained catalogue");
    }
    client.server_tool("serena", "initial_instructions", json!({}));
    let config = client.server_tool("serena", "get_current_config", json!({}));
    assert!(config.to_string().contains("workflow-fixture"));
    let body = client.server_tool("serena", "find_symbol", json!({"relative_path":"src/lib.rs","name_path_pattern":"unit_price","include_body":true,"max_matches":1,"max_answer_chars":2000}));
    assert!(body.to_string().contains("unit_price"));
    let callers = client.server_tool(
        "serena",
        "find_referencing_symbols",
        json!({"relative_path":"src/lib.rs","name_path":"unit_price","max_answer_chars":3000}),
    );
    assert!(callers.to_string().contains("total"));
    client.server_tool("serena", "replace_symbol_body", json!({"relative_path":"src/lib.rs","name_path":"unit_price","body":"pub fn unit_price() -> u32 { 8 }"}));
    assert!(
        !cargo_check(&project, &run, "introduced-defect"),
        "independent CLI oracle must reject the reported successful edit"
    );
    client.server_tool("serena", "replace_symbol_body", json!({"relative_path":"src/lib.rs","name_path":"unit_price","body":"pub fn unit_price() -> u32 { 7 }"}));
    assert!(cargo_check(&project, &run, "corrected"));

    client.server_tool("serena", "write_memory", json!({"memory_name":"pricing-route","content":"Read docs/pricing.md for the authoritative price decision and current check."}));
    let route = client.server_tool(
        "serena",
        "read_memory",
        json!({"memory_name":"pricing-route"}),
    );
    assert!(route.to_string().contains("docs/pricing.md"));
    assert_eq!(
        fs::read_to_string(project.join("docs/pricing.md")).unwrap(),
        "# Pricing\n\nConfirmed: unit price is 7. Verify with cargo test --offline.\n"
    );
    client.server_tool("serena", "write_memory", json!({"memory_name":"reference","content":"Tool link: `mem:pricing-route`. Ordinary link: [route](pricing-route.md)."}));
    client.server_tool(
        "serena",
        "rename_memory",
        json!({"old_name":"pricing-route","new_name":"pricing-owner"}),
    );
    let links = fs::read_to_string(memory.join("reference.md")).unwrap();
    assert!(links.contains("mem:pricing-owner"));
    assert!(
        links.contains("(pricing-route.md)"),
        "ordinary Markdown reference handling changed; reassess the route"
    );
    assert!(!memory.join("pricing-route.md").exists());
    assert!(memory.join("pricing-owner.md").is_file());
    fs::write(
        memory.join("reference.md"),
        links.replace("(pricing-route.md)", "(pricing-owner.md)"),
    )
    .unwrap();
    let listed = client.server_tool("serena", "list_memories", json!({}));
    assert!(!listed.to_string().contains("ignored"));
    failed_tool(
        &mut client,
        "write_memory",
        json!({"memory_name":"protected","content":"must not replace"}),
    );
    failed_tool(
        &mut client,
        "write_memory",
        json!({"memory_name":"ignored","content":"must not replace"}),
    );
    failed_tool(&mut client, "read_memory", json!({"memory_name":"ignored"}));
    assert_eq!(
        fs::read_to_string(memory.join("protected.md")).unwrap(),
        "Protected fixture decision.\n"
    );
    assert_eq!(
        fs::read_to_string(memory.join("ignored.md")).unwrap(),
        "Ignored fixture data.\n"
    );
    // Hold only an owned referencing file. The installed rename moves the
    // owner before scanning references, so a later I/O failure is non-atomic.
    client.server_tool(
        "serena",
        "write_memory",
        json!({"memory_name":"move-before-failure","content":"Recover this single owner."}),
    );
    client.server_tool(
        "serena",
        "write_memory",
        json!({"memory_name":"locked-reference","content":"`mem:move-before-failure`"}),
    );
    {
        use std::os::windows::fs::OpenOptionsExt;
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(memory.join("locked-reference.md"))
            .unwrap();
        failed_tool(
            &mut client,
            "rename_memory",
            json!({"old_name":"move-before-failure","new_name":"moved-after-failure"}),
        );
        assert!(!memory.join("move-before-failure.md").exists());
        assert_eq!(
            fs::read_to_string(memory.join("moved-after-failure.md")).unwrap(),
            "Recover this single owner."
        );
        drop(held);
    }
    assert!(
        fs::read_to_string(memory.join("locked-reference.md"))
            .unwrap()
            .contains("mem:move-before-failure")
    );
    fs::rename(
        memory.join("moved-after-failure.md"),
        memory.join("move-before-failure.md"),
    )
    .unwrap();
    assert!(memory.join("move-before-failure.md").is_file());
    assert!(memory.join("reference.md").is_file());

    client.finish();
    fs::write(run.join("qualification.json"), serde_json::to_vec_pretty(&json!({"code_oracle":"baseline pass, introduced defect rejected, correction pass","memory":"pointer, rename formats, protected/ignored operations qualified","model_calls":0,"native_E1":"pending","global_L1":"pending","partial_write_recovery":"passed","clone_worktrees":"separate deterministic test"})).unwrap()).unwrap();
}

#[test]
#[ignore = "requires an explicitly selected owned project from installed_code_and_memory_operations; no model calls"]
fn installed_readonly_boundary_on_prepared_project() {
    let executable = input("CODEGRAPH_CONSUMER_CODEX_EXE");
    let home = input("CODEGRAPH_CONSUMER_HOME");
    let output = input("CODEGRAPH_CONSUMER_OUTPUT");
    let project = input("TOOL_WORKFLOW_PREPARED_PROJECT");
    assert!(
        fs::read_to_string(project.join("Cargo.toml"))
            .unwrap()
            .contains("workflow-fixture")
    );
    assert!(
        fs::read_to_string(project.join("src/lib.rs"))
            .unwrap()
            .contains("{ 7 }")
    );
    let run = tempfile::Builder::new()
        .prefix("readonly-workflow-")
        .tempdir_in(output)
        .unwrap()
        .keep();
    println!("Private read-only evidence: {}", run.display());
    let mut client = Consumer::start(&executable, &home, &project, &run.join("existing-client"));
    client.open_thread(&project);
    client.server_tool("serena", "initial_instructions", json!({}));
    let readonly = run.join("readonly-project");
    fs::create_dir_all(readonly.join("src")).unwrap();
    fs::create_dir_all(readonly.join(".serena/memories")).unwrap();
    assert!(git(&readonly, &["init", "--quiet"]).status.success());
    fs::write(
        readonly.join("Cargo.toml"),
        "[package]\nname=\"readonly-fixture\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(readonly.join(".serena/project.yml"), "project_name: readonly-fixture\nlanguages: [rust]\nread_only: true\nfixed_tools: [initial_instructions, get_current_config, find_symbol, find_referencing_symbols, get_symbols_overview, list_memories, read_memory]\n").unwrap();
    let readonly_source = "pub fn unit_price() -> u32 { 11 }\n";
    fs::write(readonly.join("src/lib.rs"), readonly_source).unwrap();
    fs::write(
        readonly.join(".serena/memories/decision.md"),
        "Owned read-only decision.",
    )
    .unwrap();
    let mut reader = Consumer::start(&executable, &home, &readonly, &run.join("readonly-client"));
    reader.open_thread(&readonly);
    let readonly_inventory = reader.request(
        "mcpServerStatus/list",
        json!({"threadId":reader.thread,"limit":100,"detail":"toolsAndAuthOnly"}),
        90,
    );
    let tools = readonly_inventory["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "serena")
        .unwrap()["tools"]
        .as_object()
        .unwrap();
    assert!(tools.contains_key("find_symbol"));
    // A shared proxy can advertise a broad catalogue before starting the
    // project worker. Actual negative calls establish enforcement below.
    fs::write(
        run.join("advertised-tools.json"),
        serde_json::to_vec_pretty(tools).unwrap(),
    )
    .unwrap();
    reader.server_tool("serena", "initial_instructions", json!({}));
    let other_body = reader.server_tool("serena", "find_symbol", json!({"relative_path":"src/lib.rs","name_path_pattern":"unit_price","include_body":true,"max_matches":1,"max_answer_chars":2000}));
    assert!(other_body.to_string().contains("11"));
    for (name, args) in [
        (
            "replace_symbol_body",
            json!({"relative_path":"src/lib.rs","name_path":"unit_price","body":"pub fn unit_price() -> u32 { 99 }"}),
        ),
        (
            "write_memory",
            json!({"memory_name":"decision","content":"must not replace"}),
        ),
    ] {
        let denied = reader.request_envelope(
            "mcpServer/tool/call",
            json!({"threadId":reader.thread,"server":"serena","tool":name,"arguments":args}),
            90,
        );
        assert!(
            denied.get("error").is_some() || denied["result"]["isError"] == true,
            "read-only write accepted: {denied}"
        );
    }
    assert_eq!(
        fs::read_to_string(readonly.join("src/lib.rs")).unwrap(),
        readonly_source
    );
    assert_eq!(
        fs::read_to_string(readonly.join(".serena/memories/decision.md")).unwrap(),
        "Owned read-only decision."
    );
    // MCP controls do not remove native file authority on an owned target.
    fs::write(
        readonly.join("native-authorized.txt"),
        "Native write remains possible.",
    )
    .unwrap();
    let first_body = client.server_tool("serena", "find_symbol", json!({"relative_path":"src/lib.rs","name_path_pattern":"unit_price","include_body":true,"max_matches":1,"max_answer_chars":2000}));
    assert!(first_body.to_string().contains("{ 7 }"));
    reader.finish();
    client.finish();
}

#[test]
#[ignore = "requires current installed CodeGraph and an owned prepared project; deliberate bounded indexing, no model calls"]
fn installed_graph_routes_on_prepared_project() {
    let executable = input("CODEGRAPH_CONSUMER_CODEX_EXE");
    let home = input("CODEGRAPH_CONSUMER_HOME");
    let output = input("CODEGRAPH_CONSUMER_OUTPUT");
    let project = input("TOOL_WORKFLOW_PREPARED_PROJECT");
    assert!(
        fs::read_to_string(project.join("Cargo.toml"))
            .unwrap()
            .contains("workflow-fixture")
    );
    let run = tempfile::Builder::new()
        .prefix("graph-workflow-")
        .tempdir_in(output)
        .unwrap()
        .keep();
    println!("Private graph-workflow evidence: {}", run.display());
    let mut client = Consumer::start(&executable, &home, &project, &run.join("client"));
    client.open_thread(&project);
    let inventory = client.request(
        "mcpServerStatus/list",
        json!({"threadId":client.thread,"limit":100,"detail":"toolsAndAuthOnly"}),
        90,
    );
    let graph_tools = inventory["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "codegraph")
        .unwrap()["tools"]
        .as_object()
        .unwrap();
    assert!(
        graph_tools.contains_key("codegraph_status"),
        "installed CodeGraph not ready; inspect registration/build before retry"
    );
    client.tool("codegraph_status", json!({}));
    client.tool("codegraph_index", json!({}));
    let definition = client.query(&project, "unit_price", Some("src/lib.rs"));
    let callers = client.tool(
        "codegraph_callers",
        json!({"symbol":"unit_price","file":"src/lib.rs","limit":5}),
    );
    assert!(callers.to_string().contains("total"));
    let impact = client.tool(
        "codegraph_impact",
        json!({"symbol":"unit_price","file":"src/lib.rs","depth":1}),
    );
    assert!(impact.to_string().contains("total"));
    let callees = client.tool(
        "codegraph_callees",
        json!({"symbol":"total","file":"src/lib.rs","limit":5}),
    );
    assert!(callees.to_string().contains("unit_price"));
    client.tool(
        "codegraph_explore",
        json!({"query":"How does total call unit_price in src/lib.rs?","maxFiles":1}),
    );
    // A name search does not promise a semantic embedding capability. The
    // current Rust source and real entry point are the independent value oracle.
    assert!(
        fs::read_to_string(project.join("src/lib.rs"))
            .unwrap()
            .contains("quantity * unit_price()")
    );
    assert!(cargo_check(&project, &run, "graph-entrypoint"));
    let before = definition["structuredContent"]["generation"]
        .as_u64()
        .expect("completed generation");
    let extra = project.join("src/added.rs");
    assert!(!extra.exists());
    fs::write(&extra, "pub fn fresh_workflow_marker() -> u32 { 17 }\n").unwrap();
    // Wait for the existing bounded watcher, not a new sync on every query.
    let after = super::committed(
        &project,
        before,
        "fresh_workflow_marker",
        Some("src/added.rs"),
    );
    assert!(after > before);
    client.query(&project, "fresh_workflow_marker", Some("src/added.rs"));
    fs::remove_file(&extra).unwrap();
    super::committed(&project, after, "fresh_workflow_marker", None);
    client.finish();
}
