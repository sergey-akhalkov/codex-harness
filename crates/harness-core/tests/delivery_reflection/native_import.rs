//! An imported closed issue is historical data, not a new local acceptance.

use super::*;

#[test]
fn imported_accepted_closure_does_not_trigger_but_new_local_acceptance_does() {
    let temp = tempfile::tempdir().unwrap();
    let source = Board::new(&temp.path().join("source"));
    source.json(&["config", "set", "events-journal", "true", "--json"]);
    let task = source.create_task("Historical accepted work", Some(Purpose::Ordinary), None);
    let historical_binding = binding(&task, Purpose::Ordinary, "imported");
    source.set_binding(&task, &historical_binding);
    source.close(&task);

    let exported = source.run(&["export"]);
    assert!(
        exported.status.success(),
        "native export failed: {}",
        String::from_utf8_lossy(&exported.stderr)
    );
    let exported = String::from_utf8(exported.stdout).unwrap();
    let issue: Value = exported
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|issue| issue["id"] == task)
        .expect("native export must contain the accepted issue");
    assert_eq!(issue["status"], "closed");
    let input = temp.path().join("historical-issue.jsonl");
    fs::write(
        &input,
        format!("{}\n", serde_json::to_string(&issue).unwrap()),
    )
    .unwrap();

    let target = Board::new(&temp.path().join("target"));
    target.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = target.controller();
    activate(&controller, JournalPolicy::RequireExisting);

    target.json(&["import", input.to_str().unwrap(), "--json"]);
    let imported = target.json(&["show", &task, "--json"]);
    assert_eq!(imported[0]["status"], "closed");
    for (key, value) in acceptance_metadata(&historical_binding).unwrap() {
        assert_eq!(
            issue["metadata"][key.as_str()],
            value,
            "native export must preserve eligibility metadata {key}"
        );
        assert_eq!(
            imported[0]["metadata"][key.as_str()],
            value,
            "native import must preserve eligibility metadata {key}"
        );
    }
    let observed = controller.poll().unwrap();
    assert_eq!(observed.accepted, 0, "{observed:?}");
    assert!(observed.jobs.is_empty(), "{observed:?}");
    assert!(
        controller
            .stored_state()
            .unwrap()
            .unwrap()
            .records
            .is_empty(),
        "an imported closure must not enter the local reflection ledger"
    );
    if observed.recovery.is_some() {
        controller
            .rebaseline_current("native import reconciled; no local work was accepted")
            .unwrap();
    }

    // A new accepted revision of this task is eligible after a genuine local
    // reopen, acceptance metadata update and close. Import is not a permanent
    // exclusion, nor may the negative case pass by disabling all collection.
    target.reopen(&task);
    target.set_binding(&task, &binding(&task, Purpose::Ordinary, "local-followup"));
    target.close(&task);
    let local = controller.poll().unwrap();
    assert_eq!(local.accepted, 1, "{local:?}");
    assert!(local.recovery.is_none(), "{local:?}");
    assert_eq!(local.jobs.len(), 1, "{local:?}");
    assert_eq!(local.jobs[0].task_id, task);
}
