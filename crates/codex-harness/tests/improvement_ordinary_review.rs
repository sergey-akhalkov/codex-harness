//! Ordinary review consumes one retained completion through the public CLI.
#![cfg(windows)]

#[path = "fixtures/improvement_comparison_common.rs"]
mod improvement_comparison_common;
use improvement_comparison_common::*;

use harness_core::{
    build_identity,
    delivery_reflection::{Purpose, SourceIdentity},
    task_worktree::{self, AssignmentSubmission, CompletionFacts, SlotAssignmentBinding},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

const EXECUTOR_SESSION: &str = "12345678-1234-4234-8234-123456789023";
const RUN: &str = "ordinary-review-source";
const TURN: &str = "ordinary-source-turn";

#[test]
fn retained_ordinary_completion_prepares_once_without_model_dispatch() {
    let _guard = INSTALL.lock().unwrap();
    let fixture = Fixture::new("ordinary-review-entry");
    let launcher = PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"));
    fixture_build(
        &fixture.state,
        "h-build",
        &fixture.proj,
        &launcher,
        "baseline",
    );
    let prompt = fixture.root.join("ordinary-task.txt");
    let criteria = fixture.root.join("ordinary-criteria.txt");
    let config = fixture.root.join("ordinary-reviewer.toml");
    let result = fixture.root.join("ordinary-result.txt");
    fs::write(
        &prompt,
        "Inspect the frozen solution.txt and report its current state.\n",
    )
    .unwrap();
    fs::write(
        &criteria,
        "The report must accurately describe the frozen solution.txt.\n",
    )
    .unwrap();
    fs::write(&result, "Completed the requested inspection.\n").unwrap();
    fs::write(
        &config,
        "sandbox_mode = 'read-only'\napproval_policy = 'never'\nmodel = 'gpt-6-astra'\nmodel_provider = 'openai'\nmodel_reasoning_effort = 'xhigh'\n[profiles.ds]\nmodel = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    let assignment_sha256 = build_identity::hash_bytes(b"ordinary-review-assignment");
    let source = SourceIdentity {
        project_id: "ordinary-review-fixture".into(),
        beads_dir: fixture.wl.join(".beads").to_string_lossy().into_owned(),
        database: "ordinary-review-fixture".into(),
        repo_root: fixture.wl.to_string_lossy().into_owned(),
        branch: git_output(&fixture.wl, &["branch", "--show-current"]),
        commit: fixture.workload_revision.clone(),
    };
    let state_dir = task_worktree::pool_state_dir(&fixture.home, &fixture.wl).unwrap();
    let completion = task_worktree::retain_completion(
        &state_dir,
        &CompletionFacts {
            run_generation: RUN.into(),
            slot: 1,
            owner: "ordinary-review-fixture".into(),
            checkout: fixture.wl.clone(),
            task: Some("ordinary-task".into()),
            handle: Some("ordinary-task-handle".into()),
            assignment: Some(SlotAssignmentBinding {
                source,
                issue: "ordinary-task".into(),
                assignment_sha256: assignment_sha256.clone(),
                purpose: Some(Purpose::Ordinary.as_str().into()),
                purpose_source: "board".into(),
            }),
            submitted_assignment: Some(AssignmentSubmission::structured(assignment_sha256)),
            role: Some("executor".into()),
            state: "completed".into(),
            exit_code: Some(0),
            session: Some(EXECUTOR_SESSION.into()),
            turn: Some(TURN.into()),
            result: Some(result),
        },
    )
    .unwrap();
    assert!(
        completion.eligibility.eligible,
        "{:?}",
        completion.eligibility
    );

    // These are already retained action facts. Review must not rediscover a
    // mutable pool stream or start the original executor to obtain them.
    let actions = fixture.root.join("retained-actions");
    fs::create_dir(&actions).unwrap();
    let raw = format!(
        "{}\n{}\n",
        json!({"method":"item/completed","params":{"turnId":TURN,"item":{
            "id":"command-1","type":"commandExecution","command":"git status --short",
            "status":"completed","exitCode":0,"aggregatedOutput":""
        }}}),
        json!({"method":"turn/completed","params":{"turn":{"id":TURN,"status":"completed"}}})
    );
    let raw_path = actions.join(format!("{RUN}.jsonl"));
    fs::write(&raw_path, &raw).unwrap();
    fs::write(
        actions.join(format!("{RUN}.json")),
        serde_json::to_vec_pretty(&json!({
            "schema":1,"attempt":RUN,"role":"executor","session":EXECUTOR_SESSION,
            "source":raw_path,"captured":true,"source_bytes":raw.len(),
            "retained_bytes":raw.len(),"source_sha256":build_identity::hash_bytes(raw.as_bytes()),
            "truncated":false,"observed_events":2,"observed_malformed":0,
            "observed_rounds":1,"observed_turn":TURN,"note":null
        }))
        .unwrap(),
    )
    .unwrap();

    let declaration = fixture.root.join("ordinary-review.json");
    let review = fixture.root.join("ordinary-review");
    let input = json!({
        "schema":1,"review":"ordinary-review-fixture",
        "task":{"source":fixture.wl,"revision":fixture.workload_revision,
            "name":"ordinary-task","read_only":true,"writable_scope":[]},
        "task_prompt":{"path":prompt,"sha256":build_identity::hash_file(&prompt).unwrap()},
        "completion":{"codex_home":fixture.home,"run_generation":RUN,"executor_profile":"ds"},
        "lead_session":null,"action_evidence_root":actions,
        "semantic_review":{
            "schema":1,"runtime":{"state":fixture.state,"build":fixture.state.join("builds/h-build"),
                "label":"ordinary-review-runtime","upstream":fixture.upstream,
                "native_config":{"path":config,"sha256":build_identity::hash_file(&config).unwrap()},
                "private_inputs":[]},
            "profile":"ds","criteria":{"path":criteria,"sha256":build_identity::hash_file(&criteria).unwrap()},
            "sources":["solution.txt"],"max_corrections":1
        }
    });
    let input_bytes = serde_json::to_vec_pretty(&input).unwrap();
    fs::write(&declaration, &input_bytes).unwrap();
    let args = [
        "review",
        "--prepare",
        "--review",
        review.to_str().unwrap(),
        "--declaration",
        declaration.to_str().unwrap(),
    ];
    let first = fixture.improve(&args);
    assert!(
        first.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(String::from_utf8_lossy(&first.stdout).contains("improve review --resume --review"));
    assert_eq!(
        fs::read(review.join("declaration.json")).unwrap(),
        input_bytes
    );
    let state_bytes = fs::read(review.join("state.json")).unwrap();
    let state: Value = serde_json::from_slice(&state_bytes).unwrap();
    assert!(state["cursor"]["attempts"].as_array().unwrap().is_empty());
    let repeated = fixture.improve(&args);
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(fs::read(review.join("state.json")).unwrap(), state_bytes);
    assert_eq!(fs::read(&declaration).unwrap(), input_bytes);
}
