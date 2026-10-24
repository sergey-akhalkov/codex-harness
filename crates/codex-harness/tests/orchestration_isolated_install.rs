//! Isolated core/board install from this checkout, then ordinary, delegated
//! Lead and Executor starts outside the checkout. Does not touch the live user
//! home. The loop acceptance includes guidance skills, role catalogues,
//! orchestration configuration, the installed executor MCP surface, the board
//! with real evidence, two repository pools, explicit release and rollback
//! without losing unrelated configuration or archived evidence.
#![cfg(windows)]

use harness_core::{
    board_lifecycle, build_identity, core_disconnect, core_install, installation_state::PathScope,
    orchestration_config, orchestration_lifecycle, role_context, task_store, task_worktree,
};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio},
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; isolated homes only"]
fn isolated_core_and_board_install_loads_orchestration_outside_checkout() {
    let source = disk_path(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap(),
    );
    let upstream = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    let root = IsolatedRoot::new();
    let root_path = disk_path(root.path());
    eprintln!(
        "isolated orchestration evidence: {} elevated={}",
        root_path.display(),
        harness_core::task_scheduler::is_elevated().unwrap()
    );
    let build = root_path.join("build");
    let codex_home = root_path.join("codex");
    let user_home = root_path.join("user");
    let workspace = root_path.join("outside");
    fs::create_dir_all(&build).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    for name in build_identity::BINARIES {
        let from = Path::new(env!("CARGO_BIN_EXE_codex")).with_file_name(name);
        fs::copy(&from, build.join(name))
            .unwrap_or_else(|error| panic!("copy {name} from {}: {error}", from.display()));
    }
    let record = build_identity::BuildRecord {
        schema: build_identity::SCHEMA,
        source_root: source.clone(),
        source: build_identity::source_identity(&source).unwrap(),
        rustc: "isolated orchestration install".into(),
        cargo: "isolated orchestration install".into(),
        target: "x86_64-pc-windows-msvc".into(),
        profile: "release".into(),
        binaries: build_identity::BINARIES
            .iter()
            .map(|name| {
                (
                    name.to_string(),
                    build_identity::hash_file(&build.join(name)).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>(),
    };
    fs::write(
        build.join("build.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();

    let mut request = core_install::Request {
        source: source.clone(),
        build,
        codex_home: codex_home.clone(),
        user_home: user_home.clone(),
        dependency_user_home: user_home.clone(),
        upstream: Some(upstream.clone()),
        timeout: Duration::from_secs(45),
        path_scope: Some(PathScope::Process),
        goal_recovery: None,
    };
    let _path = PathRestore::capture();
    let preview = core_install::connect(&request, true).unwrap();
    assert_eq!(preview.status, "preview");
    assert!(preview.runtime.is_none());
    assert!(!user_home.join(".agents/skills/team-lead").exists());
    assert!(!codex_home.join("AGENTS.md").exists());

    // Unrelated configuration a fresh install must preserve.
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(codex_home.join("auth.json"), b"unrelated-auth").unwrap();
    fs::write(
        codex_home.join("config.toml"),
        b"model = 'unrelated-kept'\n\
          model_provider = 'unrelated'\nmodel_reasoning_effort = 'high'\n\n\
          [model_providers.unrelated]\nname = 'Unrelated'\n\
          base_url = 'http://127.0.0.1:9/v1'\nwire_api = 'responses'\n\
          requires_openai_auth = false\n",
    )
    .unwrap();
    let foreign = user_home.join(".agents/skills/foreign/SKILL.md");
    fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    fs::write(&foreign, b"---\nname: foreign\ndescription: Kept.\n---\n").unwrap();
    // 0.162.1 resolves the user farm from the real OS profile rather than
    // this synthetic home, so the foreign canary above is preservation-only
    // here. This fixture-controlled project skill lives in the catalogue's
    // case directory and carries the discovery assertions below.
    let foreign_project = workspace.join(".agents/skills/foreign-project/SKILL.md");
    fs::create_dir_all(foreign_project.parent().unwrap()).unwrap();
    fs::write(
        &foreign_project,
        b"---\nname: foreign-project\ndescription: Kept.\n---\n",
    )
    .unwrap();

    let report = core_install::connect(&request, false).unwrap();
    assert_eq!(report.status, "connected");
    assert!(report.runtime.unwrap().passed);
    let launch_path = codex_home.join("harness/native-launch.json");
    let launch: serde_json::Value =
        serde_json::from_slice(&fs::read(&launch_path).unwrap()).unwrap();
    assert_eq!(launch["schema"], 3);
    assert_eq!(
        launch["goal_recovery"], true,
        "a new install uses the bounded recovery default"
    );
    let before_preview = fs::read(&launch_path).unwrap();
    request.goal_recovery = Some(false);
    assert_eq!(
        core_install::connect(&request, true).unwrap().status,
        "preview"
    );
    assert_eq!(
        fs::read(&launch_path).unwrap(),
        before_preview,
        "a recovery-policy preview performs no writes"
    );
    assert_eq!(
        core_install::connect(&request, false).unwrap().status,
        "connected"
    );
    let launch: serde_json::Value =
        serde_json::from_slice(&fs::read(&launch_path).unwrap()).unwrap();
    assert_eq!(launch["goal_recovery"], false);
    request.goal_recovery = None;
    assert_eq!(
        core_install::connect(&request, false).unwrap().status,
        "connected"
    );
    let launch: serde_json::Value =
        serde_json::from_slice(&fs::read(&launch_path).unwrap()).unwrap();
    assert_eq!(
        launch["goal_recovery"], false,
        "an update without the option retains the explicit opt-out"
    );
    assert_eq!(
        fs::read_link(codex_home.join("AGENTS.md")).unwrap(),
        source.join("global/principles-of-work.md")
    );
    assert_eq!(
        fs::read_link(user_home.join(".agents/skills/team-lead")).unwrap(),
        source.join(".agents/skills/team-lead")
    );
    assert_eq!(
        fs::read_link(user_home.join(".agents/skills/board-workflow")).unwrap(),
        source.join(".agents/skills/board-workflow")
    );
    assert!(
        !user_home.join(".agents/skills/isolated-worktree").exists(),
        "the retired isolated-worktree skill is no longer delivered"
    );
    assert!(
        !source.join(".agents/skills/isolated-worktree").exists(),
        "the retired skill is no longer part of the kit source"
    );
    let skill = fs::read_to_string(user_home.join(".agents/skills/team-lead/SKILL.md")).unwrap();
    assert!(skill.contains("name: team-lead"));
    // A fresh session discovers the delivered workflow, not a stub: the
    // guidance it loads names the loop's board and pacing contract.
    let board_skill =
        fs::read_to_string(user_home.join(".agents/skills/board-workflow/SKILL.md")).unwrap();
    assert!(board_skill.contains("lead_review"), "{board_skill}");
    assert!(
        board_skill.contains("pacing-observation v1"),
        "{board_skill}"
    );
    assert!(board_skill.contains("route records"), "{board_skill}");
    assert!(skill.contains("vote_threshold"), "{skill}");
    assert!(skill.contains("board-workflow"), "{skill}");
    let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
    assert!(
        config.contains("model = 'unrelated-kept'"),
        "unrelated configuration survives the core install: {config}"
    );
    assert_eq!(
        fs::read_to_string(&foreign).unwrap(),
        "---\nname: foreign\ndescription: Kept.\n---\n",
        "foreign skills are preserved"
    );
    assert_eq!(
        fs::read_to_string(&foreign_project).unwrap(),
        "---\nname: foreign-project\ndescription: Kept.\n---\n",
        "the consumer project's own skill is preserved"
    );

    // The orchestration configuration names machine-local profiles; kit
    // install does not create them, so the isolated home supplies the same
    // profile files a real installation would have.
    fs::write(
        codex_home.join("ds.config.toml"),
        "model = 'deepseek-flash'\nmodel_provider = 'deepseek'\n\
         model_reasoning_effort = 'max'\n\n\
         [model_providers.deepseek]\nname = 'DeepSeek'\n\
         base_url = 'http://127.0.0.1:9/v1'\nwire_api = 'responses'\n\
         requires_openai_auth = false\n",
    )
    .unwrap();
    let delivery = orchestration_lifecycle::check(&source, &codex_home, &user_home, false).unwrap();
    eprintln!("loop delivery report: {delivery:?}");
    assert_eq!(delivery.status, "Orchestration connected");
    assert_eq!(delivery.guidance, ["team-lead", "board-workflow"]);
    assert!(delivery.guidance_missing.is_empty());
    assert_eq!(delivery.delegated_lead_profile.as_deref(), Some("ds"));
    assert_eq!(delivery.executor_profiles, ["ds"]);
    assert_eq!(delivery.director_max_leads, Some(2));
    assert_eq!(delivery.director_max_executors, Some(4));
    assert_eq!(delivery.lead_max_executors, Some(6));
    assert_eq!(
        delivery
            .roles
            .iter()
            .map(|state| (state.role, state.status))
            .collect::<Vec<_>>(),
        [
            ("lead", "resolved"),
            ("delegated-lead", "resolved"),
            ("executor", "resolved"),
            ("executor-self-reflection", "resolved"),
            ("lead-self-reflection", "resolved"),
        ],
        "every responsibility role this checkout owns resolves for delivery"
    );
    let role_revision = |role: &str| {
        delivery
            .roles
            .iter()
            .find(|state| state.role == role)
            .and_then(|state| state.revision.clone())
            .unwrap()
    };
    let lead_revision = role_revision("lead");
    let delegated_lead_revision = role_revision("delegated-lead");
    let executor_revision = role_revision("executor");
    let executor_reflection_revision = role_revision("executor-self-reflection");
    let lead_reflection_revision = role_revision("lead-self-reflection");
    assert!(!lead_revision.is_empty());
    assert!(!delegated_lead_revision.is_empty());
    assert!(!executor_revision.is_empty());
    assert!(!executor_reflection_revision.is_empty());
    assert!(!lead_reflection_revision.is_empty());
    let mut revisions = delivery
        .roles
        .iter()
        .filter_map(|state| state.revision.clone())
        .collect::<Vec<_>>();
    let role_count = revisions.len();
    revisions.sort();
    revisions.dedup();
    assert_eq!(
        revisions.len(),
        role_count,
        "each role revision identifies its distinct owned inputs"
    );
    let roles_of = |report: &orchestration_lifecycle::CheckReport| {
        report
            .roles
            .iter()
            .map(|state| (state.role, state.status, state.revision.clone()))
            .collect::<Vec<_>>()
    };
    let delivered_roles = roles_of(&delivery);
    let configured = orchestration_config::load(&source).unwrap();
    assert_eq!(configured.lead_profile, "default");
    assert_eq!(configured.successor_lead_profile, "default");
    assert_eq!(configured.executor_profiles, ["ds"]);
    assert_eq!(
        delivery.max_concurrent_executors,
        Some(configured.max_concurrent_executors)
    );
    assert_eq!(
        delivery.worktree_pool_size,
        Some(orchestration_config::worktree_pool_size(&configured))
    );
    let director_capacity = delivery.director_executor_capacity.as_ref().unwrap();
    assert_eq!(director_capacity.parent_limit, 4);
    assert_eq!(director_capacity.effective_limit, 4);
    assert_eq!(director_capacity.bottlenecks, ["parent allowance"]);
    let lead_capacity = delivery.lead_executor_capacity.as_ref().unwrap();
    assert_eq!(lead_capacity.parent_limit, 6);
    assert_eq!(lead_capacity.effective_limit, 6);
    assert_eq!(
        delivery.board_version, None,
        "the board is a separate component"
    );
    assert_eq!(delivery.model_calls, 0);
    assert!(!delivery.mutated);

    for (role, required) in [
        (
            role_context::Role::ExecutorSelfReflection,
            [
                "bounded, read-only retrospective over one accepted executor",
                "Do not edit files, the board, guidance, specifications or configuration",
                "Return one compact synthesis",
            ],
        ),
        (
            role_context::Role::LeadSelfReflection,
            [
                "bounded, read-only synthesis over one lead conversation",
                "executor self-reflection retained from a completed task",
                "Do not edit files, the board, guidance, specifications or configuration",
            ],
        ),
    ] {
        let resolved = role_context::resolve(&source, role).unwrap();
        for text in required {
            assert!(
                resolved.developer_instructions.contains(text),
                "{} omits its read-only responsibility: {text}",
                role.label()
            );
        }
    }

    let board = board_lifecycle::Request {
        source: source.clone(),
        codex_home: codex_home.clone(),
        user_home: user_home.clone(),
        preview: true,
    };
    let board_preview = board_lifecycle::install(&board).unwrap();
    assert_eq!(board_preview.status, "Preview board Install");
    assert!(!codex_home.join("harness/bin/bd.exe").exists());
    let board_on = board_lifecycle::Request {
        preview: false,
        ..board
    };
    let board_report = board_lifecycle::install(&board_on).unwrap();
    assert_eq!(board_report.status, "Board connected");
    assert!(codex_home.join("harness/bin/bd.exe").is_file());
    let connected =
        orchestration_lifecycle::check(&source, &codex_home, &user_home, false).unwrap();
    assert_eq!(
        connected.board_version.as_deref(),
        Some(board_report.bd_version.as_deref().unwrap()),
        "the delivered board is visible to the loop check"
    );
    let bd = codex_home.join("harness/bin/bd.exe");
    let version =
        String::from_utf8_lossy(&run(&bd, &workspace, &["--version"]).stdout).into_owned();
    assert!(version.contains("1.3.0"), "{version}");

    // Real board evidence through the installed tool, then read it back after
    // rollback: archived loop evidence must outlive the installation.
    let project = root_path.join("loop-project");
    fs::create_dir_all(&project).unwrap();
    board_git(&project, &["init", "-q"]);
    board_git(&project, &["config", "user.email", "loop@example.test"]);
    board_git(&project, &["config", "user.name", "Loop Fixture"]);
    let init = run(
        &bd,
        &project,
        &[
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "loop",
        ],
    );
    assert!(init.status.success(), "{}", failed("init", &init));
    let epic = json(
        &bd,
        &project,
        &["create", "Stage: loop evidence", "--type", "epic", "--json"],
    );
    let epic_id = field(&epic, "id");
    let item = json(
        &bd,
        &project,
        &[
            "create",
            "Loop evidence item",
            "--type",
            "task",
            "--parent",
            &epic_id,
            "--json",
        ],
    );
    let item_id = field(&item, "id");
    let closed = run(
        &bd,
        &project,
        &[
            "close",
            &item_id,
            "--reason",
            "archived: synthetic evidence",
        ],
    );
    assert!(closed.status.success(), "{}", failed("close", &closed));

    prepare_user_profile(&user_home);
    let version = Command::new(codex_home.join("harness/bin/codex.exe"))
        .current_dir(&workspace)
        .env("CODEX_HOME", &codex_home)
        .env("USERPROFILE", &user_home)
        .env("HOME", &user_home)
        .env("APPDATA", user_home.join("AppData/Roaming"))
        .env("LOCALAPPDATA", user_home.join("AppData/Local"))
        .env("TEMP", user_home.join("AppData/Local/Temp"))
        .env("TMP", user_home.join("AppData/Local/Temp"))
        .args(["--version"])
        .output()
        .unwrap();
    assert!(
        version.status.success(),
        "ordinary installed launcher failed outside checkout: {}{}",
        String::from_utf8_lossy(&version.stderr),
        String::from_utf8_lossy(&version.stdout)
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&version.stdout),
        String::from_utf8_lossy(&version.stderr)
    );
    assert!(
        text.contains("codex"),
        "ordinary session must start the installed launcher: {text}"
    );

    // A version response is not context consumption. Inspect a fresh native
    // context in the outside project through the installed launcher as well.
    let project_guidance = "Owned outside-project instruction marker";
    fs::write(workspace.join("AGENTS.md"), project_guidance).unwrap();
    let context = outside_prompt(&codex_home, &user_home, &workspace, None);
    for relative in ["global/principles-of-work.md", "global/roles/lead.md"] {
        let expected = fs::read_to_string(source.join(relative))
            .unwrap()
            .replace("\r\n", "\n");
        assert!(
            context.contains(expected.trim()),
            "outside context omits {relative}"
        );
    }
    assert!(
        context.contains(project_guidance),
        "outside project guidance is missing"
    );
    let executor_guidance = fs::read_to_string(source.join("global/roles/executor.md"))
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        !context.contains(executor_guidance.trim()),
        "lead loaded the executor role"
    );
    assert!(context.contains("danger-full-access"));
    assert!(context.contains("Approval policy is currently never"));

    // The executor side of the same installed launcher: the dispatch marker
    // and declared executor profile select the executor role, so a second
    // fresh context must carry the shared core and executor fragment, omit
    // the lead-only fragment and keep the project guidance.
    let executor_context = outside_prompt(&codex_home, &user_home, &workspace, Some("executor"));
    for relative in ["global/principles-of-work.md", "global/roles/executor.md"] {
        let expected = fs::read_to_string(source.join(relative))
            .unwrap()
            .replace("\r\n", "\n");
        assert!(
            executor_context.contains(expected.trim()),
            "executor context omits {relative}"
        );
    }
    assert!(
        executor_context.contains(project_guidance),
        "executor project guidance is missing"
    );
    let lead_guidance = fs::read_to_string(source.join("global/roles/lead.md"))
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        !executor_context.contains(lead_guidance.trim()),
        "executor loaded the lead role"
    );

    // The delegated Lead is a distinct bounded coordinator: the profile stays
    // ds, the root-only lead guidance is not composed, and no executor marker
    // or role is inherited from the calling session.
    let delegated_context =
        outside_prompt(&codex_home, &user_home, &workspace, Some("delegated-lead"));
    let delegated_guidance = fs::read_to_string(source.join("global/roles/delegated-lead.md"))
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        delegated_context.contains(delegated_guidance.trim()),
        "delegated lead context omits its role fragment"
    );
    for relative in ["global/roles/lead.md", "global/roles/executor.md"] {
        let guidance = fs::read_to_string(source.join(relative))
            .unwrap()
            .replace("\r\n", "\n");
        assert!(
            !delegated_context.contains(guidance.trim()),
            "delegated lead loaded {relative}"
        );
    }
    assert!(
        delegated_context.contains(project_guidance),
        "delegated lead project guidance is missing"
    );

    // The installed catalogue path resolves the same recorded source for
    // both roles: the owned revision is reported, the lead-only skill is
    // disabled only for the executor, and the unrelated project skill stays
    // in both effective sets.
    let catalogue = |args: &[&str]| -> String {
        let out = Command::new(codex_home.join("harness/bin/codex-harness.exe"))
            .current_dir(&workspace)
            .env("CODEX_HOME", &codex_home)
            .env("USERPROFILE", &user_home)
            .env("HOME", &user_home)
            .env("APPDATA", user_home.join("AppData/Roaming"))
            .env("LOCALAPPDATA", user_home.join("AppData/Local"))
            .env("TEMP", user_home.join("AppData/Local/Temp"))
            .env("TMP", user_home.join("AppData/Local/Temp"))
            .args(["skills", "catalogue", "--limit", "1048576"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", failed("skills catalogue", &out));
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };
    let lead_catalogue = catalogue(&["--role", "lead"]);
    assert!(
        lead_catalogue.starts_with(&format!(
            "role_context=role=lead;profile=none;revision={lead_revision}\n"
        )),
        "{lead_catalogue}"
    );
    let executor_catalogue = catalogue(&["--role", "executor", "--profile", "ds"]);
    assert!(
        executor_catalogue.starts_with(&format!(
            "role_context=role=executor;profile=ds;revision={executor_revision}\n"
        )),
        "{executor_catalogue}"
    );
    let delegated_catalogue = catalogue(&["--role", "delegated-lead", "--profile", "ds"]);
    assert!(
        delegated_catalogue.starts_with(&format!(
            "role_context=role=delegated-lead;profile=ds;revision={delegated_lead_revision}\n"
        )),
        "{delegated_catalogue}"
    );
    let enabled = |text: &str, name: &str, scope: &str| -> bool {
        let prefix = format!("name={name};scope={scope};");
        text.lines()
            .find(|line| line.starts_with(&prefix))
            .unwrap_or_else(|| panic!("catalogue omits {name} in scope {scope}: {text}"))
            .contains(";enabled=true;")
    };
    assert!(enabled(&lead_catalogue, "team-lead", "user"));
    assert!(enabled(&lead_catalogue, "board-workflow", "user"));
    assert!(enabled(&lead_catalogue, "foreign-project", "repo"));
    assert!(!enabled(&executor_catalogue, "team-lead", "user"));
    assert!(enabled(&executor_catalogue, "board-workflow", "user"));
    assert!(enabled(&executor_catalogue, "foreign-project", "repo"));
    assert!(!enabled(&delegated_catalogue, "team-lead", "user"));
    assert!(enabled(&delegated_catalogue, "board-workflow", "user"));
    assert!(enabled(&delegated_catalogue, "foreign-project", "repo"));

    // Reflection skills are delivered for their own lifecycle-selected roles.
    for reflection in ["executor-self-reflection", "lead-self-reflection"] {
        assert!(
            source
                .join(".agents/skills")
                .join(reflection)
                .join("SKILL.md")
                .is_file(),
            "reflection role skill {reflection} is not delivered"
        );
        assert!(
            user_home
                .join(".agents/skills")
                .join(reflection)
                .join("SKILL.md")
                .is_file(),
            "installed user root omits reflection role skill {reflection}"
        );
    }

    // The installed launcher outside the checkout exposes the candidate
    // acceptance and lifecycle commands. Every call below uses only owned
    // process-scoped homes, the synthetic board and no provider or model.
    let installed_harness = codex_home.join("harness/bin/codex-harness.exe");
    let project_text = project.to_str().unwrap();
    let home_text = codex_home.to_str().unwrap();
    let installed = |args: &[&str], thread: Option<&str>| -> Output {
        let mut command = Command::new(&installed_harness);
        command
            .current_dir(&workspace)
            .env("CODEX_HOME", &codex_home)
            .env("USERPROFILE", &user_home)
            .env("HOME", &user_home)
            .env("APPDATA", user_home.join("AppData/Roaming"))
            .env("LOCALAPPDATA", user_home.join("AppData/Local"))
            .env("TEMP", user_home.join("AppData/Local/Temp"))
            .env("TMP", user_home.join("AppData/Local/Temp"))
            .env_remove("HARNESS_EXECUTOR_SESSION")
            .env_remove("HARNESS_REFLECTION_SESSION")
            .args(args);
        match thread {
            Some(thread) => {
                command.env("CODEX_THREAD_ID", thread);
            }
            None => {
                command.env_remove("CODEX_THREAD_ID");
            }
        }
        command.output().unwrap()
    };

    // The installed plugin server is the actual route surface. Both dispatch
    // tools are declared, and an absent or unregistered caller identity is
    // refused before a pool position, receipt, endpoint or thread exists.
    let mut mcp = InstalledMcp::start(
        &installed_harness,
        &codex_home,
        &user_home,
        &root_path.join("installed-mcp.stderr.txt"),
    );
    let listed = mcp.request(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    }));
    let tools = listed["result"]["tools"].as_array().unwrap();
    let names = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    for required in [
        "lead_dispatch",
        "executor_dispatch",
        "executor_status",
        "executor_wait",
        "executor_resume",
        "executor_release",
    ] {
        assert!(
            names.contains(&required),
            "installed tools omit {required}: {listed}"
        );
    }
    let lead_dispatch = tools
        .iter()
        .find(|tool| tool["name"] == "lead_dispatch")
        .unwrap();
    assert!(
        lead_dispatch["description"]
            .as_str()
            .unwrap()
            .contains("Director-only"),
        "{lead_dispatch}"
    );
    assert!(
        lead_dispatch["inputSchema"]["properties"]["assignment"]["description"]
            .as_str()
            .unwrap()
            .contains("OpenSpec"),
        "{lead_dispatch}"
    );
    let assignment = serde_json::json!({
        "schema": 1,
        "objective": "refuse an unregistered installed caller identity",
        "inputs": [],
        "outputs": [],
        "invariants": [],
        "acceptance": ["no pool, receipt, endpoint or thread is created"]
    });
    let missing = mcp.call_tool(
        3,
        "executor_dispatch",
        serde_json::json!({"owner": "installed-identity", "assignment": assignment.clone()}),
        None,
    );
    assert!(
        mcp_error(&missing).contains("_meta.threadId"),
        "absent identity was not refused: {missing}"
    );
    let forged = mcp.call_tool(
        4,
        "lead_dispatch",
        serde_json::json!({"owner": "installed-forged", "assignment": assignment.clone()}),
        Some(serde_json::json!({
            "threadId": "01a00000-0000-7000-8000-000000000000"
        })),
    );
    assert!(
        mcp_error(&forged).contains("no lead endpoint is registered"),
        "unregistered identity was not refused: {forged}"
    );
    for relative in ["harness/executor-pool", "harness/messages"] {
        let path = codex_home.join(relative);
        assert!(
            !path.exists() || fs::read_dir(&path).unwrap().next().is_none(),
            "a refused identity created state at {}",
            path.display()
        );
    }
    mcp.finish();

    let help = installed(&["executor", "accept", "--help"], None);
    assert!(
        help.status.success(),
        "{}",
        failed_output("installed executor accept --help", &help)
    );
    let help_text = format!(
        "{}{}",
        String::from_utf8_lossy(&help.stdout),
        String::from_utf8_lossy(&help.stderr)
    )
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ");
    for required in [
        "Accept retained structured work as the live immediate parent",
        "TASK is the exact",
        "writes the acceptance binding through the board owner and closes",
        "Acceptance never releases a workspace",
        "Repeating the same accepted result is a no-op",
    ] {
        assert!(
            help_text.contains(required),
            "installed accept help omits {required:?}: {help_text}"
        );
    }

    let installed_check = installed(
        &[
            "check",
            "--core-only",
            "--codex-home",
            home_text,
            "--user-home",
            user_home.to_str().unwrap(),
            "--dependency-user-home",
            user_home.to_str().unwrap(),
        ],
        None,
    );
    assert!(
        installed_check.status.success(),
        "{}",
        failed_output("installed core check", &installed_check)
    );
    let installed_check: serde_json::Value =
        serde_json::from_slice(&installed_check.stdout).unwrap();
    assert_eq!(installed_check["status"], "connected");
    assert_eq!(installed_check["model_calls"], 0);
    assert_eq!(installed_check["goal_recovery"]["configured"], false);
    assert_eq!(installed_check["goal_recovery"]["support"], "declared");
    assert_eq!(installed_check["goal_recovery"]["exercised"], false);
    let orchestration = &installed_check["orchestration"];
    assert_eq!(orchestration["status"], "Orchestration connected");
    assert_eq!(orchestration["model_calls"], 0);
    assert_eq!(orchestration["mutated"], false);
    assert_eq!(orchestration["lead_profile"], "default");
    assert_eq!(orchestration["successor_lead_profile"], "default");
    assert_eq!(orchestration["delegated_lead_profile"], "ds");
    assert_eq!(orchestration["director_max_leads"], 2);
    assert_eq!(orchestration["director_max_executors"], 4);
    assert_eq!(orchestration["lead_max_executors"], 6);
    assert_eq!(orchestration["max_concurrent_executors"], 16);
    assert_eq!(orchestration["worktree_pool_size"], 18);
    assert_eq!(
        orchestration["director_executor_capacity"]["effective_limit"],
        4
    );
    assert_eq!(
        orchestration["director_executor_capacity"]["bottlenecks"],
        serde_json::json!(["parent allowance"])
    );
    assert_eq!(
        orchestration["lead_executor_capacity"]["effective_limit"],
        6
    );
    assert_eq!(
        orchestration["executor_profiles"],
        serde_json::json!(["ds"])
    );
    let expected_role_revisions = delivery
        .roles
        .iter()
        .map(|state| {
            (
                state.role.to_owned(),
                state.revision.clone().expect("resolved fixture role"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let installed_roles = orchestration["roles"].as_array().unwrap();
    assert_eq!(
        installed_roles.len(),
        expected_role_revisions.len(),
        "the installed check reports every declared responsibility role"
    );
    let installed_role_revisions = installed_roles
        .iter()
        .map(|state| {
            assert_eq!(state["status"], "resolved", "{state}");
            assert!(state["cause"].is_null(), "{state}");
            (
                state["role"].as_str().unwrap().to_owned(),
                state["revision"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        installed_role_revisions, expected_role_revisions,
        "installed role resolution matches the owned source revisions"
    );

    // Repeat status is model-free and creates no retained lifecycle. Missing
    // inputs refuse before journal configuration or board effects.
    let state_dir = codex_home.join("harness/delivery-reflection");
    assert!(
        !state_dir.exists(),
        "the fixture starts without retained reflection state"
    );
    let status_args = [
        "delivery-improvement",
        "status",
        "--project",
        project_text,
        "--codex-home",
        home_text,
    ];
    for repeat in 0..2 {
        let status = installed(&status_args, None);
        assert!(
            status.status.success(),
            "{}",
            failed_output("installed delivery-improvement status", &status)
        );
        let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
        assert_eq!(status["lifecycle"], "inactive", "repeat {repeat}: {status}");
        assert_eq!(status["pending"], 0, "repeat {repeat}: {status}");
        assert_eq!(
            status["selfReflectionEnabled"], true,
            "inactive status reports no invented lifecycle state: {status}"
        );
        assert_eq!(status["modelWork"], "stopped", "repeat {repeat}: {status}");
        assert!(
            status["modelWorkReason"]
                .as_str()
                .unwrap()
                .contains("no reflection lifecycle"),
            "repeat {repeat}: {status}"
        );
        assert!(
            !state_dir.exists(),
            "repeat status must not create retained reflection state"
        );
    }
    let enable_args = [
        "delivery-improvement",
        "enable",
        "--project",
        project_text,
        "--codex-home",
        home_text,
    ];
    let missing_thread = installed(&enable_args, None);
    assert_eq!(
        missing_thread.status.code(),
        Some(2),
        "{}",
        failed_output(
            "installed delivery-improvement enable without a lead",
            &missing_thread
        )
    );
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&missing_thread.stdout),
            String::from_utf8_lossy(&missing_thread.stderr)
        )
        .contains("CODEX_THREAD_ID is missing"),
        "{}",
        failed_output(
            "installed delivery-improvement enable without a lead",
            &missing_thread
        )
    );
    let unavailable_lead = installed(&enable_args, Some("isolated-missing-lead"));
    assert_eq!(
        unavailable_lead.status.code(),
        Some(2),
        "{}",
        failed_output(
            "installed delivery-improvement enable with an absent lead",
            &unavailable_lead
        )
    );
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&unavailable_lead.stdout),
            String::from_utf8_lossy(&unavailable_lead.stderr)
        )
        .contains("the real lead is unavailable"),
        "{}",
        failed_output(
            "installed delivery-improvement enable with an absent lead",
            &unavailable_lead
        )
    );
    assert!(
        !state_dir.exists(),
        "refused enable must not configure a journal or retain reflection state"
    );

    let issue = |id: &str| -> serde_json::Value {
        let value = json(&bd, &project, &["show", id, "--json"]);
        value
            .as_array()
            .and_then(|items| items.first())
            .cloned()
            .unwrap_or(value)
    };
    let before_accept = issue(&epic_id);
    assert_eq!(before_accept["status"], "open");
    let refused = installed(
        &[
            "executor",
            "accept",
            &epic_id,
            "--project",
            project_text,
            "--codex-home",
            home_text,
        ],
        None,
    );
    assert_eq!(
        refused.status.code(),
        Some(2),
        "{}",
        failed_output("installed executor accept without a live lead", &refused)
    );
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&refused.stdout),
            String::from_utf8_lossy(&refused.stderr)
        )
        .contains("CODEX_THREAD_ID is missing or blank"),
        "{}",
        failed_output("installed executor accept without a live lead", &refused)
    );
    let after_accept = issue(&epic_id);
    assert_eq!(
        after_accept["status"], "open",
        "accept refusal leaves the synthetic task open"
    );
    assert_eq!(
        after_accept["metadata"], before_accept["metadata"],
        "accept refusal records no acceptance metadata"
    );

    board_lifecycle::disconnect(&board_on).unwrap();
    assert!(
        !codex_home.join("harness/bin/bd.exe").exists(),
        "rollback removes the board link"
    );
    let package = codex_home
        .join("harness/board/packages")
        .join(board_report.bd_version.as_deref().unwrap())
        .join("bd.exe");
    assert!(package.is_file(), "the pinned board package stays in place");
    let evidence = json(&package, &project, &["show", &item_id, "--json"]);
    let evidence = evidence
        .as_array()
        .and_then(|items| items.first())
        .cloned()
        .unwrap_or(evidence);
    assert_eq!(field(&evidence, "status"), "closed");
    assert!(
        field(&evidence, "close_reason").contains("archived: synthetic evidence"),
        "archived board evidence survives rollback: {evidence}"
    );

    // A real held task and its checkout outlive removal of the installation.
    let task_repo = root_path.join("retained-task");
    fs::create_dir_all(&task_repo).unwrap();
    board_git(&task_repo, &["init", "-q"]);
    board_git(&task_repo, &["config", "user.email", "task@example.test"]);
    board_git(&task_repo, &["config", "user.name", "Task Fixture"]);
    fs::write(task_repo.join(".gitignore"), b"cache/\n").unwrap();
    fs::write(task_repo.join("owned.txt"), b"initial\n").unwrap();
    board_git(&task_repo, &["add", ".gitignore", "owned.txt"]);
    board_git(&task_repo, &["commit", "-qm", "owned task base"]);
    // Enter through the installed CLI in a consumer with no kit configuration.
    // Source and capacity must come from the installation, not this fixture.
    let acquired = run(
        &codex_home.join("harness/bin/codex-harness.exe"),
        &task_repo,
        &[
            "worktree",
            "acquire",
            "--codex-home",
            codex_home.to_str().unwrap(),
            "--repository",
            task_repo.to_str().unwrap(),
            "--task",
            "retained-work",
            "--base",
            "HEAD",
        ],
    );
    assert!(
        acquired.status.success(),
        "{}",
        failed("worktree acquire", &acquired)
    );
    let pool_line = String::from_utf8_lossy(&acquired.stdout).into_owned();
    assert!(
        pool_line.contains("capacity=18"),
        "the installed configuration supplies the explicit pool capacity: {pool_line}"
    );
    assert!(
        pool_line.contains("account=configured("),
        "capacity comes from the installed configuration, not a fallback: {pool_line}"
    );
    let held =
        task_worktree::continue_task(&codex_home, &task_repo, "retained-work", None).unwrap();
    fs::write(held.path.join("owned.txt"), b"partial tracked work\n").unwrap();
    fs::write(held.path.join("new.txt"), b"partial untracked work\n").unwrap();
    fs::create_dir_all(held.path.join("cache")).unwrap();
    fs::write(held.path.join("cache/ready"), b"prepared\n").unwrap();

    // The board stays on the coordinating repository; the pinned installed
    // board package cannot resolve that issue from an independent repository.
    let foreign_board = run(&package, &task_repo, &["show", &item_id, "--json"]);
    assert!(
        !foreign_board.status.success(),
        "an independent repository unexpectedly resolved the coordinating board: {}",
        failed_output("foreign board show", &foreign_board)
    );

    // A second independent repository enters through the same installed pool
    // owner. Its clean release returns the exact base and a stale generation
    // handle cannot release whatever occupies the position later.
    let consumer = root_path.join("integration-consumer");
    fs::create_dir_all(&consumer).unwrap();
    board_git(&consumer, &["init", "-q"]);
    board_git(
        &consumer,
        &["config", "user.email", "consumer@example.test"],
    );
    board_git(&consumer, &["config", "user.name", "Consumer Fixture"]);
    fs::write(consumer.join("consumer.txt"), b"consumer base\n").unwrap();
    board_git(&consumer, &["add", "consumer.txt"]);
    board_git(&consumer, &["commit", "-qm", "consumer base"]);
    let consumer_base = git_head(&consumer);
    let acquired = run(
        &installed_harness,
        &consumer,
        &[
            "worktree",
            "acquire",
            "--codex-home",
            home_text,
            "--repository",
            consumer.to_str().unwrap(),
            "--task",
            "consumer-work",
            "--base",
            "HEAD",
        ],
    );
    assert!(
        acquired.status.success(),
        "{}",
        failed("second-repository worktree acquire", &acquired)
    );
    let consumer_holding =
        task_worktree::continue_task(&codex_home, &consumer, "consumer-work", None).unwrap();
    let consumer_handle = consumer_holding.handle.clone().unwrap();
    let released = run(
        &installed_harness,
        &consumer,
        &[
            "worktree",
            "release",
            "--codex-home",
            home_text,
            "--repository",
            consumer.to_str().unwrap(),
            "--task",
            "consumer-work",
            "--handle",
            &consumer_handle,
            "--disposition",
            "discarded",
            "--reason",
            "isolated installed acceptance",
        ],
    );
    assert!(
        released.status.success(),
        "{}",
        failed("second-repository worktree release", &released)
    );
    let release_text = String::from_utf8_lossy(&released.stdout).into_owned();
    assert!(
        release_text.contains("disposition=discarded released"),
        "{release_text}"
    );
    assert!(
        release_text.contains(&format!("reset to {consumer_base}")),
        "{release_text}"
    );
    assert_eq!(
        git_head(&consumer_holding.path),
        consumer_base,
        "the released second-repository slot returns to its committed base"
    );
    let stale = run(
        &installed_harness,
        &consumer,
        &[
            "worktree",
            "release",
            "--codex-home",
            home_text,
            "--repository",
            consumer.to_str().unwrap(),
            "--task",
            "consumer-work",
            "--handle",
            &consumer_handle,
            "--disposition",
            "discarded",
            "--reason",
            "stale generation",
        ],
    );
    assert_eq!(
        stale.status.code(),
        Some(2),
        "{}",
        failed("stale worktree release", &stale)
    );
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&stale.stdout),
            String::from_utf8_lossy(&stale.stderr)
        )
        .contains("nothing was released"),
        "{}",
        failed_output("stale worktree release", &stale)
    );

    // An installed update while the task is held keeps the binding, its
    // partial work and its ignored cache; the retired skill stays absent.
    let updated = core_install::connect(&request, false).unwrap();
    assert_eq!(updated.status, "connected");
    assert!(
        !user_home.join(".agents/skills/isolated-worktree").exists(),
        "an update does not re-advertise the retired skill"
    );
    let update_roles =
        roles_of(&orchestration_lifecycle::check(&source, &codex_home, &user_home, false).unwrap());
    assert_eq!(
        update_roles, delivered_roles,
        "an installed update keeps both roles resolving from the recorded sources"
    );
    let after_update = task_worktree::continue_task(
        &codex_home,
        &task_repo,
        "retained-work",
        held.handle.as_deref(),
    )
    .unwrap();
    assert_eq!(after_update.path, held.path);
    assert_eq!(after_update.base, held.base);
    assert_eq!(after_update.handle, held.handle);
    for (relative, expected) in [
        ("owned.txt", "partial tracked work\n"),
        ("new.txt", "partial untracked work\n"),
        ("cache/ready", "prepared\n"),
    ] {
        assert_eq!(
            fs::read_to_string(held.path.join(relative)).unwrap(),
            expected,
            "an installed update must preserve {relative}"
        );
    }

    // Conflicting ownership: a different requested base for the held task is
    // refused before any reset and the held files survive the refusal.
    fs::write(task_repo.join("owned.txt"), b"next base\n").unwrap();
    board_git(&task_repo, &["add", "owned.txt"]);
    board_git(&task_repo, &["commit", "-qm", "next base"]);
    let next_base = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&task_repo)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    let conflicted = run(
        &codex_home.join("harness/bin/codex-harness.exe"),
        &task_repo,
        &[
            "worktree",
            "acquire",
            "--codex-home",
            codex_home.to_str().unwrap(),
            "--repository",
            task_repo.to_str().unwrap(),
            "--task",
            "retained-work",
            "--base",
            next_base.as_str(),
        ],
    );
    assert_eq!(
        conflicted.status.code(),
        Some(2),
        "{}",
        failed("conflicting-base acquire", &conflicted)
    );
    let stderr = String::from_utf8_lossy(&conflicted.stderr).into_owned();
    assert!(stderr.contains("conflicts with the binding"), "{stderr}");
    for (relative, expected) in [
        ("owned.txt", "partial tracked work\n"),
        ("new.txt", "partial untracked work\n"),
        ("cache/ready", "prepared\n"),
    ] {
        assert_eq!(
            fs::read_to_string(held.path.join(relative)).unwrap(),
            expected,
            "a refused conflicting base must preserve {relative}"
        );
    }

    // Unsupported record versions: an ownership schema this build does not
    // support refuses with the record bytes intact, and restoring the record
    // recovers the same binding. The discriminator is the protection an older
    // host sees during rollback: it refuses the record instead of treating the
    // held task as a free expired session.
    let record_path = task_worktree::slot_record_path(&codex_home, &task_repo, held.index).unwrap();
    let record = fs::read(&record_path).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&record).unwrap();
    assert_eq!(
        value["ownershipSchema"], 2,
        "new records publish the protected discriminator"
    );
    value["ownershipSchema"] = serde_json::json!(999);
    let unsupported = serde_json::to_vec(&value).unwrap();
    fs::write(&record_path, &unsupported).unwrap();
    let refused = run(
        &codex_home.join("harness/bin/codex-harness.exe"),
        &task_repo,
        &[
            "worktree",
            "continue",
            "--codex-home",
            codex_home.to_str().unwrap(),
            "--repository",
            task_repo.to_str().unwrap(),
            "--task",
            "retained-work",
        ],
    );
    assert_eq!(
        refused.status.code(),
        Some(2),
        "{}",
        failed("unsupported-record continue", &refused)
    );
    let stderr = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(stderr.contains("declares ownership schema 999"), "{stderr}");
    assert_eq!(
        fs::read(&record_path).unwrap(),
        unsupported,
        "the refused record is preserved byte-for-byte"
    );
    fs::write(&record_path, &record).unwrap();
    let recovered = task_worktree::continue_task(
        &codex_home,
        &task_repo,
        "retained-work",
        held.handle.as_deref(),
    )
    .unwrap();
    assert_eq!(recovered.path, held.path);
    assert_eq!(recovered.handle, held.handle);

    let disconnected =
        core_disconnect::disconnect(&codex_home, &user_home, &user_home, false).unwrap();
    assert_eq!(disconnected.status, "disconnected");
    assert!(
        disconnected
            .orchestration
            .preserved
            .contains(&"executor-pool")
    );
    let continued = task_worktree::continue_task(
        &codex_home,
        &task_repo,
        "retained-work",
        held.handle.as_deref(),
    )
    .unwrap();
    assert_eq!(continued.path, held.path);
    assert_eq!(continued.base, held.base);
    assert_eq!(continued.handle, held.handle);
    for (relative, expected) in [
        ("owned.txt", "partial tracked work\n"),
        ("new.txt", "partial untracked work\n"),
        ("cache/ready", "prepared\n"),
    ] {
        assert_eq!(
            fs::read_to_string(continued.path.join(relative)).unwrap(),
            expected,
            "disconnect must preserve {relative}"
        );
    }
    assert!(
        disconnected.orchestration.preserved.contains(&"skills"),
        "the skill source stays readable for the next install"
    );
    assert!(
        !user_home.join(".agents/skills/team-lead").exists(),
        "rollback removes the loop guidance links"
    );
    assert!(user_home.join(".agents/skills/foreign/SKILL.md").is_file());
    assert!(
        foreign_project.is_file(),
        "rollback must not touch the consumer project's own skill"
    );
    // Role inputs resolve from the recorded source root, not a deployed copy:
    // rollback removes the owned links but leaves role resolution intact, and
    // the reported revisions stay those of the owned sources.
    let after_rollback =
        orchestration_lifecycle::check(&source, &codex_home, &user_home, false).unwrap();
    assert_eq!(roles_of(&after_rollback), delivered_roles);
    assert_eq!(
        after_rollback.guidance_missing,
        ["team-lead", "board-workflow"]
    );
    let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
    assert!(
        config.contains("model = 'unrelated-kept'"),
        "unrelated configuration survives rollback: {config}"
    );
    assert_eq!(
        fs::read(codex_home.join("auth.json")).unwrap(),
        b"unrelated-auth",
        "credentials are unrelated configuration"
    );
    eprintln!(
        "rollback preserved: {:?}",
        disconnected.orchestration.preserved
    );
}

#[test]
fn isolated_lifecycle_check_reports_fresh_prior_and_overridden_hierarchy() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let config_path = source.join("global/orchestration.toml");
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    fs::write(
        &config_path,
        include_bytes!("../../../global/orchestration.toml"),
    )
    .unwrap();
    let codex_home = root.path().join("codex");
    let user_home = root.path().join("user");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("ds.config.toml"),
        "model = 'deepseek-flash'\n",
    )
    .unwrap();
    fs::write(
        codex_home.join("lead-alt.config.toml"),
        "model = 'alternate'\n",
    )
    .unwrap();

    let fresh = orchestration_lifecycle::check(&source, &codex_home, &user_home, false).unwrap();
    assert_eq!(fresh.lead_profile.as_deref(), Some("default"));
    assert_eq!(fresh.successor_lead_profile.as_deref(), Some("default"));
    assert_eq!(fresh.delegated_lead_profile.as_deref(), Some("ds"));
    assert_eq!(fresh.director_max_leads, Some(2));
    assert_eq!(fresh.director_max_executors, Some(4));
    assert_eq!(fresh.lead_max_executors, Some(6));
    assert_eq!(
        fresh
            .director_executor_capacity
            .as_ref()
            .unwrap()
            .effective_limit,
        4
    );
    assert_eq!(
        fresh
            .lead_executor_capacity
            .as_ref()
            .unwrap()
            .effective_limit,
        6
    );

    // A configuration from before hierarchy settings keeps its root route,
    // gains only the materialized pool setting, and reports compatible
    // defaults without writing user choices.
    let prior = br#"schema = 1
lead_profile = "default"
successor_lead_profile = "default"
executor_profiles = ["ds"]
max_concurrent_executors = 6
vote_threshold = 3
incubator_size_cap = 32
feedback_batch_limit = 8
worktree_limit = 6
"#;
    fs::write(&config_path, prior).unwrap();
    assert_eq!(
        orchestration_lifecycle::materialize_pool_capacity(&source).unwrap(),
        Some(6)
    );
    let prior_report =
        orchestration_lifecycle::check(&source, &codex_home, &user_home, false).unwrap();
    assert_eq!(prior_report.lead_profile.as_deref(), Some("default"));
    assert_eq!(
        prior_report.successor_lead_profile.as_deref(),
        Some("default")
    );
    assert_eq!(prior_report.delegated_lead_profile.as_deref(), Some("ds"));
    assert_eq!(prior_report.director_max_leads, Some(2));
    assert_eq!(prior_report.director_max_executors, Some(4));
    assert_eq!(prior_report.lead_max_executors, Some(6));
    assert_eq!(prior_report.worktree_pool_size, Some(6));

    let overridden = br#"schema = 1
lead_profile = "default"
successor_lead_profile = "default"
delegated_lead_profile = "lead-alt"
executor_profiles = ["ds"]
director_max_leads = 1
director_max_executors = 3
lead_max_executors = 5
max_concurrent_executors = 9
worktree_pool_size = 11
"#;
    fs::write(&config_path, overridden).unwrap();
    let override_report =
        orchestration_lifecycle::check(&source, &codex_home, &user_home, false).unwrap();
    assert_eq!(override_report.lead_profile.as_deref(), Some("default"));
    assert_eq!(
        override_report.successor_lead_profile.as_deref(),
        Some("default")
    );
    assert_eq!(
        override_report.delegated_lead_profile.as_deref(),
        Some("lead-alt")
    );
    assert_eq!(override_report.director_max_leads, Some(1));
    assert_eq!(override_report.director_max_executors, Some(3));
    assert_eq!(override_report.lead_max_executors, Some(5));
    assert_eq!(override_report.max_concurrent_executors, Some(9));
    assert_eq!(override_report.worktree_pool_size, Some(11));
    assert_eq!(
        override_report
            .director_executor_capacity
            .as_ref()
            .unwrap()
            .effective_limit,
        3
    );
    assert_eq!(
        override_report
            .lead_executor_capacity
            .as_ref()
            .unwrap()
            .effective_limit,
        5
    );
    assert_eq!(fs::read(&config_path).unwrap(), overridden);
}

#[test]
fn isolated_lifecycle_check_rejects_non_positive_hierarchy_limits() {
    for field in [
        "director_max_leads",
        "director_max_executors",
        "lead_max_executors",
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let config_path = source.join("global/orchestration.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        let config = format!(
            "schema = 1\nlead_profile = \"default\"\n\
             successor_lead_profile = \"default\"\nexecutor_profiles = [\"ds\"]\n\
             max_concurrent_executors = 6\n{field} = 0\n"
        );
        fs::write(&config_path, &config).unwrap();
        let codex_home = root.path().join("codex");
        fs::create_dir_all(&codex_home).unwrap();
        fs::write(
            codex_home.join("ds.config.toml"),
            "model = 'deepseek-flash'\n",
        )
        .unwrap();
        let user_home = root.path().join("user");

        let error =
            orchestration_lifecycle::check(&source, &codex_home, &user_home, false).unwrap_err();
        assert!(error.to_string().contains(field), "{field}: {error}");
        assert!(
            error.to_string().contains("must be a positive integer"),
            "{field}: {error}"
        );
        assert_eq!(fs::read_to_string(&config_path).unwrap(), config);
        assert!(!user_home.exists(), "{field} refusal must not write");
    }
}

#[test]
fn isolated_rollback_preserves_legacy_run_and_refuses_unsupported_hierarchy_record() {
    let root = tempfile::tempdir().unwrap();
    let codex_home = root.path().join("codex");
    let user_home = root.path().join("user");
    let repository = root.path().join("retained-hierarchy");
    fs::create_dir_all(&repository).unwrap();
    board_git(&repository, &["init", "-q"]);
    board_git(
        &repository,
        &["config", "user.email", "hierarchy@example.test"],
    );
    board_git(&repository, &["config", "user.name", "Hierarchy Fixture"]);
    fs::write(repository.join("owned.txt"), b"base\n").unwrap();
    board_git(&repository, &["add", "owned.txt"]);
    board_git(&repository, &["commit", "-qm", "hierarchy base"]);

    let held = task_worktree::acquire_task(
        &codex_home,
        &repository,
        2,
        "retained-hierarchy",
        None,
        &|_| false,
    )
    .unwrap();
    fs::write(held.path.join("partial.txt"), b"preserve\n").unwrap();
    let record_path =
        task_worktree::slot_record_path(&codex_home, &repository, held.index).unwrap();
    let record = fs::read(&record_path).unwrap();
    let mut unsupported: serde_json::Value = serde_json::from_slice(&record).unwrap();
    unsupported["ownershipSchema"] = serde_json::json!(999);
    let unsupported = serde_json::to_vec(&unsupported).unwrap();
    fs::write(&record_path, &unsupported).unwrap();
    let refused =
        task_worktree::continue_task(&codex_home, &repository, "retained-hierarchy", None)
            .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("declares ownership schema 999"),
        "{refused}"
    );

    let legacy = task_store::TaskRecord {
        schema: 1,
        id: "legacy-run".into(),
        workspace: repository.clone(),
        worktree: Some(held.path.clone()),
        lead_profile: "default".into(),
        authorization: "full".into(),
        requirements: "synthetic retained run".into(),
        stopped: true,
        active_lead: "default".into(),
        purpose: None,
        assignments: vec![task_store::Assignment {
            id: "legacy-executor".into(),
            profile: "ds".into(),
            owner_thread: Some("legacy-thread".into()),
            worktree: Some(held.path.clone()),
            attempt: 1,
            accepted: None,
            defect: None,
            purpose: None,
        }],
    };
    task_store::save(&codex_home, &legacy).unwrap();

    let preserved =
        orchestration_lifecycle::disconnect_preserves_private(&codex_home, &user_home).unwrap();
    for expected in ["task-store", "executor-pool"] {
        assert!(
            preserved.preserved.contains(&expected),
            "rollback omitted {expected}: {:?}",
            preserved.preserved
        );
    }
    assert_eq!(
        fs::read(&record_path).unwrap(),
        unsupported,
        "rollback must retain the unsupported hierarchy record byte-for-byte"
    );
    assert_eq!(
        fs::read(held.path.join("partial.txt")).unwrap(),
        b"preserve\n"
    );
    let retained = task_store::load(&codex_home, "legacy-run").unwrap();
    assert_eq!(retained.schema, 1);
    assert!(retained.stopped);
    assert_eq!(retained.assignments.len(), 1);

    fs::write(&record_path, &record).unwrap();
    let continued =
        task_worktree::continue_task(&codex_home, &repository, "retained-hierarchy", None).unwrap();
    assert_eq!(continued.path, held.path);
}

#[test]
fn tracked_executor_plugin_names_both_routes_and_preserves_the_server_command() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(repository.join(".agents/plugins/executors/.codex-plugin/plugin.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["mcpServers"], "./.mcp.json");
    let long = manifest["interface"]["longDescription"].as_str().unwrap();
    for required in ["lead_dispatch", "executor_dispatch", "Director", "Lead"] {
        assert!(long.contains(required), "plugin interface omits {required}");
    }
    assert!(
        long.contains("One shared plugin server"),
        "plugin interface must not imply a plugin identity per thread"
    );
    let mcp: serde_json::Value = serde_json::from_slice(
        &fs::read(repository.join(".agents/plugins/executors/.mcp.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(mcp["mcpServers"]["executors"]["command"], "codex-harness");
    assert_eq!(
        mcp["mcpServers"]["executors"]["args"],
        serde_json::json!(["mcp", "executors"])
    );
}

/// Minimal JSON-lines MCP client for the installed executors server. The server
/// owns no app-server until a trusted caller identity arrives, so this client
/// can verify the installed surface and refusal paths without model work.
struct InstalledMcp {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    stderr_path: PathBuf,
}

impl InstalledMcp {
    fn start(harness: &Path, codex_home: &Path, user_home: &Path, stderr_path: &Path) -> Self {
        let stderr = fs::File::create(stderr_path).unwrap();
        let mut child = Command::new(harness)
            .args(["mcp", "executors"])
            .env("CODEX_HOME", codex_home)
            .env("USERPROFILE", user_home)
            .env("HOME", user_home)
            .env("APPDATA", user_home.join("AppData/Roaming"))
            .env("LOCALAPPDATA", user_home.join("AppData/Local"))
            .env("TEMP", user_home.join("AppData/Local/Temp"))
            .env("TMP", user_home.join("AppData/Local/Temp"))
            .env_remove("HARNESS_EXECUTOR_SESSION")
            .env_remove("HARNESS_CONTEXT_ROLE")
            .env_remove("HARNESS_REFLECTION_SESSION")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut server = Self {
            child,
            stdin,
            stdout,
            stderr_path: stderr_path.to_owned(),
        };
        let initialized = server.request(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "installed-acceptance", "version": "1"}
            }
        }));
        assert_eq!(
            initialized["result"]["serverInfo"]["name"], "executors",
            "{initialized}"
        );
        server.send(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
        server
    }

    fn send(&mut self, value: serde_json::Value) {
        let mut line = serde_json::to_vec(&value).unwrap();
        line.push(b'\n');
        self.stdin.as_mut().unwrap().write_all(&line).unwrap();
        self.stdin.as_mut().unwrap().flush().unwrap();
    }

    fn request(&mut self, value: serde_json::Value) -> serde_json::Value {
        self.send(value);
        let mut line = String::new();
        if self.stdout.read_line(&mut line).unwrap() == 0 {
            panic!(
                "installed executors MCP exited before replying: {}",
                self.stderr()
            );
        }
        serde_json::from_str(&line).unwrap()
    }

    fn call_tool(
        &mut self,
        id: u64,
        tool: &str,
        arguments: serde_json::Value,
        metadata: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let mut params = serde_json::json!({"name": tool, "arguments": arguments});
        if let Some(metadata) = metadata {
            params["_meta"] = metadata;
        }
        self.request(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": params
        }))
    }

    fn stderr(&self) -> String {
        fs::read_to_string(&self.stderr_path).unwrap_or_default()
    }

    fn finish(mut self) {
        drop(self.stdin.take());
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "installed executors MCP stderr: {}",
                    self.stderr()
                );
                return;
            }
            if Instant::now() >= until {
                let _ = self.child.kill();
                panic!("installed executors MCP did not exit: {}", self.stderr());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

fn mcp_error(response: &serde_json::Value) -> String {
    assert_eq!(response["result"]["isError"], true, "{response}");
    response["result"]["structuredContent"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("tool error missing: {response}"))
        .to_owned()
}

/// One fresh native context from the installed launcher outside the checkout:
/// the ordinary root session, a delegated Lead, or the Executor session that
/// the dispatch marker selects. Ambient role markers are cleared first so the
/// fixture, not the calling session, selects the composed role.
fn outside_prompt(
    codex_home: &Path,
    user_home: &Path,
    workspace: &Path,
    role: Option<&str>,
) -> String {
    let mut command = Command::new(codex_home.join("harness/bin/codex.exe"));
    command
        .current_dir(workspace)
        .env("CODEX_HOME", codex_home)
        .env("USERPROFILE", user_home)
        .env("HOME", user_home)
        .env("APPDATA", user_home.join("AppData/Roaming"))
        .env("LOCALAPPDATA", user_home.join("AppData/Local"))
        .env("TEMP", user_home.join("AppData/Local/Temp"))
        .env("TMP", user_home.join("AppData/Local/Temp"))
        .env_remove("HARNESS_EXECUTOR_SESSION")
        .env_remove("HARNESS_CONTEXT_ROLE");
    match role {
        None => {}
        Some("executor") => {
            command
                .env("HARNESS_EXECUTOR_SESSION", "1")
                .args(["--profile", "ds"]);
        }
        Some("delegated-lead") => {
            command
                .env("HARNESS_CONTEXT_ROLE", "delegated-lead")
                .args(["--profile", "ds"]);
        }
        Some(role) => panic!("unsupported fixture role {role}"),
    }
    let prompt = command.args(["debug", "prompt-input"]).output().unwrap();
    assert!(
        prompt.status.success(),
        "{}",
        failed("outside prompt", &prompt)
    );
    let messages: serde_json::Value = serde_json::from_slice(&prompt.stdout).unwrap();
    messages
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|message| message["content"].as_array().unwrap())
        .filter_map(|content| content["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .replace("\r\n", "\n")
}

fn run(program: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .env("BD_NON_INTERACTIVE", "1")
        .env("BEADS_ACTOR", "isolated-loop-check")
        .output()
        .unwrap()
}

fn json(program: &Path, cwd: &Path, args: &[&str]) -> serde_json::Value {
    let out = run(program, cwd, args);
    assert!(out.status.success(), "{}", failed(args[0], &out));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn field(value: &serde_json::Value, name: &str) -> String {
    value
        .get(name)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("missing field {name} in {value}"))
        .to_owned()
}

fn failed(command: &str, out: &Output) -> String {
    format!(
        "bd {command} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn failed_output(command: &str, out: &Output) -> String {
    format!(
        "{command} exited {}: {}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn board_git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_head(cwd: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn disk_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text.as_ref()))
}

fn prepare_user_profile(user: &Path) {
    for dir in [
        user.join("AppData/Roaming"),
        user.join("AppData/Local/Temp"),
        user.join(".agents/skills"),
    ] {
        fs::create_dir_all(dir).unwrap();
    }
}

struct PathRestore(Option<std::ffi::OsString>);
impl PathRestore {
    fn capture() -> Self {
        Self(std::env::var_os("PATH"))
    }
}

/// Owned isolated homes. Set `HARNESS_KEEP_ISOLATED_EVIDENCE` to retain the
/// whole fixture, including rollback preservation evidence, after the run.
struct IsolatedRoot(Option<tempfile::TempDir>);

impl IsolatedRoot {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("osa-isolated-")
            .tempdir()
            .unwrap();
        Self(Some(dir))
    }

    fn path(&self) -> &Path {
        self.0.as_ref().unwrap().path()
    }
}

impl Drop for IsolatedRoot {
    fn drop(&mut self) {
        if std::env::var_os("HARNESS_KEEP_ISOLATED_EVIDENCE").is_none() {
            return;
        }
        if let Some(dir) = self.0.take() {
            eprintln!(
                "retained isolated evidence: {}",
                disk_path(&dir.keep()).display()
            );
        }
    }
}
impl Drop for PathRestore {
    fn drop(&mut self) {
        unsafe {
            match &self.0 {
                Some(path) => std::env::set_var("PATH", path),
                None => std::env::remove_var("PATH"),
            }
        }
    }
}
