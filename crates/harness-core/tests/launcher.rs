//! Accepted script-launcher dispatch cases, ported as argument-vector oracles.
use harness_core::launcher::{
    NativePreferences, additional_roots, daemon_opt_out, executor_limited, per_model_effort,
    profile_arguments, task_arguments, xai_shim_requested,
};
use std::ffi::OsString;

fn argv(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn executor_launches_keep_the_agent_capability_off() {
    let plain = argv(&["exec", "hello"]);
    assert_eq!(executor_limited(plain.clone(), false), plain);
    assert_eq!(
        executor_limited(plain, true),
        argv(&["-c", "agents.enabled=false", "exec", "hello"])
    );
    let already_limited = argv(&["-c", "agents.enabled=false", "exec", "hello"]);
    assert_eq!(
        executor_limited(already_limited.clone(), true),
        already_limited
    );
}

#[test]
fn native_profile_dispatch_preserves_the_accepted_commands_and_boundaries() {
    let cases: &[(&[&str], bool)] = &[
        (&[], true),
        (&["hello there"], true),
        (&["exec", "hello"], true),
        (&["e", "hello"], true),
        (&["exec", "help", "resume"], false),
        (&["exec", "resume", "--last", "help"], true),
        (&["-hV"], false),
        (&["review", "--uncommitted"], true),
        (&["resume", "--last"], true),
        (&["fork", "--last"], true),
        (&["debug", "prompt-input", "hello"], true),
        (&["-c", "note=\"a b\"", "exec", "hello"], true),
        (&["-c", "-profile", "exec"], true),
        (&["-cnote=\"a b\"", "exec"], true),
        (&["--config", "note=\"exec\"", "mcp", "list"], false),
        (&["--config=note=\"a b\"", "exec"], true),
        (&["--profile", "other", "exec"], false),
        (&["exec", "-p", "other"], false),
        (&["-pother", "exec"], false),
        (&["exec", "--profile=other"], false),
        (&["exec", "--", "--profile", "literal"], true),
        (&["--", "mcp"], true),
        (&["exec", "--", "--help"], true),
        (&["--image", "one.png", "mcp"], true),
        (
            &["--image", "one.png", "--config", "x=1", "mcp", "list"],
            false,
        ),
        (&["debug", "-c", "note=\"models\"", "prompt-input"], true),
        (&["debug", "models"], false),
        (&["debug", "app-server"], false),
        (&["--remote", "ws://localhost:9999"], false),
        (&["--remote=ws://localhost:9999"], false),
    ];
    for (input, select) in cases {
        let mut expected = if *select {
            argv(&["--profile", "harness"])
        } else {
            Vec::new()
        };
        expected.extend(argv(input));
        assert_eq!(profile_arguments(&argv(input)), expected, "{input:?}");
    }
    for command in [
        "agents",
        "login",
        "logout",
        "mcp",
        "plugin",
        "mcp-server",
        "app-server",
        "remote-control",
        "app",
        "completion",
        "update",
        "doctor",
        "sandbox",
        "apply",
        "a",
        "queue",
        "archive",
        "delete",
        "migrate-rollouts",
        "unarchive",
        "cloud",
        "exec-server",
        "features",
        "help",
    ] {
        assert_eq!(profile_arguments(&argv(&[command])), argv(&[command]));
    }
    for flag in ["-h", "--help", "-V", "--version"] {
        for input in [argv(&[flag]), argv(&["exec", flag])] {
            assert_eq!(profile_arguments(&input), input);
        }
    }
}

#[test]
fn xai_shim_starts_only_for_the_explicit_xai_profile() {
    assert!(xai_shim_requested(&argv(&["--profile", "xai"])));
    assert!(xai_shim_requested(&argv(&[
        "exec",
        "--profile",
        "xai",
        "hi"
    ])));
    assert!(xai_shim_requested(&argv(&["--profile=xai"])));
    assert!(xai_shim_requested(&argv(&["-p", "xai", "exec"])));
    assert!(!xai_shim_requested(&argv(&[])));
    assert!(!xai_shim_requested(&argv(&["--profile", "zai"])));
    assert!(!xai_shim_requested(&argv(&["--profile", "harness"])));
    assert!(!xai_shim_requested(&argv(&[
        "exec",
        "--",
        "--profile",
        "xai"
    ])));
}

#[test]
fn task_effort_does_not_interpret_prompts_or_override_native_settings() {
    let prompt = argv(&[
        "exec",
        "-c",
        "message=\"some spaces and quoted text\"",
        "--",
        "",
        "путь с пробелами",
        "quote\"inside",
        "C:\\trailing slash\\",
        "--profile",
        "line 1\nline 2",
    ]);
    for (name, effort) in [
        ("routine", "low"),
        ("standard", "high"),
        ("demanding", "xhigh"),
    ] {
        let mut input = argv(&["--harness-effort", name]);
        input.extend(prompt.clone());
        let mut expected = argv(&[
            "--profile",
            "harness",
            "-c",
            &format!("model_reasoning_effort=\"{effort}\""),
        ]);
        expected.extend(prompt.clone());
        assert_eq!(
            profile_arguments(&task_arguments(&input).unwrap()),
            expected
        );
    }
    for rest in [
        argv(&["-c", "model_reasoning_effort=\"max\"", "exec", "hello"]),
        argv(&["--config=model_reasoning_effort=\"max\"", "exec"]),
        argv(&["-cmodel_reasoning_effort=\"max\"", "exec"]),
        argv(&["--profile", "personal", "exec", "hello"]),
        argv(&["--remote=ws://localhost:9999"]),
    ] {
        let mut input = argv(&["--harness-effort=routine"]);
        input.extend(rest.clone());
        assert_eq!(task_arguments(&input).unwrap(), rest);
    }
    let literal = argv(&["exec", "--", "--harness-effort", "routine"]);
    assert_eq!(task_arguments(&literal).unwrap(), literal);
    for input in [
        argv(&["--harness-effort"]),
        argv(&["--harness-effort="]),
        argv(&["--harness-effort", "invalid"]),
    ] {
        assert!(task_arguments(&input).is_err());
    }
    // An image path resembling an option value never changes the native effort.
    let input = argv(&[
        "--harness-effort=routine",
        "--image=a.png",
        "model_reasoning_effort=max",
    ]);
    assert_eq!(
        &task_arguments(&input).unwrap()[..2],
        argv(&["-c", "model_reasoning_effort=\"low\""])
    );
}

#[test]
fn per_model_effort_defaults_without_explicit_selection() {
    for (model, effort) in [
        ("xai/grok-4.6", "xhigh"),
        ("gpt-6-astra", "xhigh"),
        ("zai/glm-5.3", "max"),
    ] {
        let input = argv(&["-m", model, "exec", "hello"]);
        let mut expected = argv(&["-c", &format!("model_reasoning_effort=\"{effort}\"")]);
        expected.extend(input.clone());
        assert_eq!(per_model_effort(&input, resolved(None)), expected);
    }
    let plain = argv(&["exec", "hello"]);
    let mut expected = argv(&["-c", "model_reasoning_effort=\"max\""]);
    expected.extend(plain.clone());
    assert_eq!(
        per_model_effort(&plain, resolved(Some("zai/glm-5.3"))),
        expected
    );
    assert_eq!(
        per_model_effort(&argv(&["-m", "other/model", "exec"]), resolved(None)),
        argv(&["-m", "other/model", "exec"])
    );
    assert_eq!(
        per_model_effort(&argv(&["mcp", "list"]), resolved(Some("zai/glm-5.3"))),
        argv(&["mcp", "list"])
    );
}

fn resolved<'a>(model: Option<&'a str>) -> NativePreferences<'a> {
    NativePreferences {
        resolved: true,
        model,
        effort: None,
    }
}

#[test]
fn per_model_effort_respects_explicit_selections() {
    for rest in [
        argv(&[
            "-c",
            "model_reasoning_effort=\"low\"",
            "-m",
            "zai/glm-5.3",
            "exec",
        ]),
        argv(&[
            "--config=model_reasoning_effort=\"low\"",
            "-m",
            "zai/glm-5.3",
        ]),
        argv(&["-cmodel_reasoning_effort=\"low\"", "-m", "zai/glm-5.3"]),
        argv(&["--model=zai/glm-5.3", "--profile", "personal", "exec"]),
        argv(&["--remote=ws://localhost:9999", "-m", "zai/glm-5.3"]),
    ] {
        assert_eq!(
            per_model_effort(&rest, resolved(Some("xai/grok-4.6"))),
            rest
        );
    }
    let input = argv(&["--harness-effort=routine", "-m", "zai/glm-5.3"]);
    assert_eq!(per_model_effort(&input, resolved(None)), input);
    let input = argv(&["-c", "model=\"xai/grok-4.6\"", "exec"]);
    let mut expected = argv(&["-c", "model_reasoning_effort=\"xhigh\""]);
    expected.extend(input.clone());
    assert_eq!(per_model_effort(&input, resolved(None)), expected);
}

#[test]
fn per_model_effort_yields_to_applicable_native_configuration() {
    // A saved native model/effort pair is the effective session choice: the
    // harness mapping must not replace the lighter effort with a CLI
    // override, whatever the model's per-model default would be.
    let saved = argv(&["exec", "hello"]);
    assert_eq!(
        per_model_effort(
            &saved,
            NativePreferences {
                resolved: true,
                model: Some("zai/glm-5.3"),
                effort: Some("low"),
            }
        ),
        saved
    );
    // The same applies when the model is only known from configuration and
    // the effort comes from a trusted project layer.
    assert_eq!(
        per_model_effort(
            &saved,
            NativePreferences {
                resolved: true,
                model: None,
                effort: Some("medium"),
            }
        ),
        saved
    );
    // An unresolved configuration is an unknown native choice: no fallback is
    // promoted over it, even when the invocation names a mapped model.
    assert_eq!(
        per_model_effort(
            &argv(&["-m", "zai/glm-5.3", "exec"]),
            NativePreferences {
                resolved: false,
                model: None,
                effort: None,
            }
        ),
        argv(&["-m", "zai/glm-5.3", "exec"])
    );
}

#[test]
fn canonical_native_effort_needs_no_legacy_selector() {
    let resolved = NativePreferences {
        resolved: true,
        model: None,
        effort: None,
    };
    // The native configuration is the canonical interface: both functions
    // leave it untouched, including beside a model with a per-model default.
    let native = argv(&[
        "-c",
        "model_reasoning_effort=low",
        "-m",
        "zai/glm-5.3",
        "exec",
        "hello",
    ]);
    assert_eq!(task_arguments(&native).unwrap(), native);
    assert_eq!(per_model_effort(&native, resolved), native);

    // A translated compatibility selector is already explicit native
    // configuration, so no per-model default stacks on top of it.
    let translated = task_arguments(&argv(&[
        "--harness-effort=routine",
        "-m",
        "zai/glm-5.3",
        "exec",
    ]))
    .unwrap();
    assert_eq!(
        translated,
        argv(&[
            "-c",
            "model_reasoning_effort=\"low\"",
            "-m",
            "zai/glm-5.3",
            "exec"
        ])
    );
    assert_eq!(per_model_effort(&translated, resolved), translated);

    // The `--model=` spelling reaches the same per-model default.
    let selected = argv(&["--model=zai/glm-5.3", "exec"]);
    let mut expected = argv(&["-c", "model_reasoning_effort=\"max\""]);
    expected.extend(selected.clone());
    assert_eq!(per_model_effort(&selected, resolved), expected);
}

#[test]
fn executor_launches_keep_the_translated_or_profiled_effort() {
    // The executor route composes the same translation, per-model default and
    // capability switch: the selector wins over the mapped model's default.
    let translated = per_model_effort(
        &task_arguments(&argv(&[
            "--harness-effort=standard",
            "-m",
            "zai/glm-5.3",
            "exec",
            "work",
        ]))
        .unwrap(),
        resolved(None),
    );
    assert_eq!(
        executor_limited(translated, true),
        argv(&[
            "-c",
            "agents.enabled=false",
            "-c",
            "model_reasoning_effort=\"high\"",
            "-m",
            "zai/glm-5.3",
            "exec",
            "work"
        ])
    );
    // A configured profile route stays authoritative: the selector yields and
    // no per-model default is injected.
    let routed = per_model_effort(
        &task_arguments(&argv(&[
            "--harness-effort=routine",
            "--profile",
            "ds",
            "exec",
        ]))
        .unwrap(),
        resolved(Some("zai/glm-5.3")),
    );
    assert_eq!(
        executor_limited(routed, true),
        argv(&["-c", "agents.enabled=false", "--profile", "ds", "exec"])
    );
}

#[test]
fn embedded_tui_sessions_opt_out_of_daemon_auto_start() {
    // Live shared defaults and explicit profiles always run codex-cli's
    // app-server embedded; the config-level opt-out keeps that session while
    // suppressing the unused shared-daemon fallback warning.
    let opt_out = argv(&["-c", "features.daemon_auto_start=false"]);
    for (args, shared) in [
        (argv(&["--profile", "zai"]), false),
        (argv(&["--profile=zai"]), false),
        (argv(&["-p", "zai"]), false),
        (argv(&[]), true),
        (argv(&["hello there"]), true),
        (argv(&["resume", "--last"]), true),
        (argv(&["fork", "--last"]), true),
    ] {
        assert_eq!(daemon_opt_out(&args, shared), opt_out, "{args:?}");
    }
    for (args, shared) in [
        // Without kit layers the session keeps codex-cli's daemon discovery.
        (argv(&[]), false),
        (argv(&["--remote", "ws://127.0.0.1:1"]), false),
        (argv(&["--remote", "ws://127.0.0.1:1"]), true),
        (argv(&["--no-daemon"]), true),
        (argv(&["--no-daemon", "--profile", "zai"]), false),
        // Non-TUI commands neither run the daemon startup path nor accept
        // broad opt-outs everywhere.
        (argv(&["exec", "hello"]), true),
        (argv(&["e", "hello"]), true),
        (argv(&["review", "--uncommitted"]), true),
        (argv(&["debug", "prompt-input", "hello"]), true),
        (argv(&["mcp", "list"]), true),
        (argv(&["agents"]), true),
        (argv(&["--help"]), true),
        (argv(&["-V"]), false),
    ] {
        assert!(daemon_opt_out(&args, shared).is_empty(), "{args:?}");
    }
}

#[test]
fn explicit_roots_use_effective_cwd_without_prompt_or_remote_expansion() {
    let temp = tempfile::tempdir().unwrap();
    let primary = temp.path().join("primary");
    let additional = temp.path().join("additional root кириллица");
    std::fs::create_dir(&primary).unwrap();
    std::fs::create_dir(&additional).unwrap();
    let input = vec![
        "-C".into(),
        primary.into_os_string(),
        "--add-dir".into(),
        "../additional root кириллица".into(),
        format!("--add-dir={}", additional.display()).into(),
        "exec".into(),
        "hello".into(),
    ];
    assert_eq!(
        additional_roots(&input, temp.path()),
        vec![additional.canonicalize().unwrap()]
    );
    for input in [
        argv(&["exec", "--", "--add-dir", "."]),
        argv(&["-c", "--add-dir", "exec"]),
        argv(&["--remote=ws://localhost:9999", "--add-dir", "."]),
        argv(&["--add-dir"]),
    ] {
        assert!(additional_roots(&input, temp.path()).is_empty());
    }
}
