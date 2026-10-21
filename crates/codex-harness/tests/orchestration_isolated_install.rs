//! Isolated core/board install from this checkout, then an ordinary Codex
//! start outside the checkout. Does not touch the live user home. The loop
//! acceptance is included: guidance skills, orchestration configuration, the
//! board tool with real board evidence, and rollback that removes the loop
//! without losing unrelated configuration or archived evidence.
#![cfg(windows)]

use harness_core::{
    board_lifecycle, build_identity, core_disconnect, core_install, installation_state::PathScope,
    orchestration_config, orchestration_lifecycle, role_context, task_worktree,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Duration,
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
    eprintln!("isolated orchestration evidence: {}", root_path.display());
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

    let request = core_install::Request {
        source: source.clone(),
        build,
        codex_home: codex_home.clone(),
        user_home: user_home.clone(),
        dependency_user_home: user_home.clone(),
        upstream: Some(upstream.clone()),
        timeout: Duration::from_secs(45),
        path_scope: Some(PathScope::Process),
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
        b"model = 'unrelated-kept'\n",
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
        "model = 'deepseek-flash'\n",
    )
    .unwrap();
    let delivery = orchestration_lifecycle::check(&source, &codex_home, &user_home, false).unwrap();
    eprintln!("loop delivery report: {delivery:?}");
    assert_eq!(delivery.status, "Orchestration connected");
    assert_eq!(delivery.guidance, ["team-lead", "board-workflow"]);
    assert!(delivery.guidance_missing.is_empty());
    assert_eq!(delivery.executor_profiles, ["ds"]);
    assert_eq!(
        delivery
            .roles
            .iter()
            .map(|state| (state.role, state.status))
            .collect::<Vec<_>>(),
        [
            ("lead", "resolved"),
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
    let executor_revision = role_revision("executor");
    let executor_reflection_revision = role_revision("executor-self-reflection");
    let lead_reflection_revision = role_revision("lead-self-reflection");
    assert!(!lead_revision.is_empty());
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
    let context = outside_prompt(&codex_home, &user_home, &workspace, false);
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
    let executor_context = outside_prompt(&codex_home, &user_home, &workspace, true);
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

    // Drafted reflection skills stay outside ordinary discovery until their
    // evaluation gate accepts them: neither role's catalogue may expose them.
    for candidate in ["executor-self-reflection", "lead-self-reflection"] {
        assert!(
            source
                .join(".agents/skill-candidates")
                .join(candidate)
                .join("SKILL.md")
                .is_file(),
            "candidate {candidate} is retained as an inactive draft"
        );
        assert!(
            !source.join(".agents/skills").join(candidate).exists()
                && !user_home.join(".agents/skills").join(candidate).exists(),
            "candidate {candidate} must not be installed into a discovery root"
        );
        let listed = format!("name={candidate};");
        assert!(
            !lead_catalogue.lines().any(|line| line.starts_with(&listed))
                && !executor_catalogue
                    .lines()
                    .any(|line| line.starts_with(&listed)),
            "catalogue exposed the unevaluated candidate {candidate}"
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
        "Accept retained structured executor work as the live lead",
        "TASK is the exact",
        "writes the acceptance binding through the board owner and closes",
        "reports whether automatic reflection is eligible",
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
    let orchestration = &installed_check["orchestration"];
    assert_eq!(orchestration["status"], "Orchestration connected");
    assert_eq!(orchestration["model_calls"], 0);
    assert_eq!(orchestration["mutated"], false);
    assert_eq!(orchestration["lead_profile"], "default");
    assert_eq!(orchestration["successor_lead_profile"], "default");
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
            status.as_object().unwrap().len(),
            2,
            "inactive status reports no invented lifecycle state: {status}"
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
        pool_line.contains("capacity=4"),
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

/// One fresh native context from the installed launcher outside the checkout:
/// the ordinary lead session, or the executor session the dispatch marker and
/// declared executor profile select. The marker is cleared explicitly first,
/// so an ambient executor session cannot change which role is composed.
fn outside_prompt(codex_home: &Path, user_home: &Path, workspace: &Path, executor: bool) -> String {
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
        .env_remove("HARNESS_EXECUTOR_SESSION");
    if executor {
        command
            .env("HARNESS_EXECUTOR_SESSION", "1")
            .args(["--profile", "ds"]);
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
