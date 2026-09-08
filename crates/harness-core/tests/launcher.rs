//! Accepted script-launcher dispatch cases, ported as argument-vector oracles.
use harness_core::launcher::{additional_roots, profile_arguments, task_arguments};
use std::ffi::OsString;

fn argv(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
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
