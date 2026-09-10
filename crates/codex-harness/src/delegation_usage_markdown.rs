//! Public-friendly projection: no transcript text, raw IDs, paths or hashes.
use super::*;

fn display(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

fn public_project(value: &Value) -> String {
    let Some(label) = value.as_str() else {
        return "unknown".into();
    };
    format!(
        "workspace-{}",
        &format!("{:x}", Sha256::digest(label.as_bytes()))[..8]
    )
}

pub(super) fn render(report: &Value) -> String {
    let totals = &report["totals"];
    let responses = &report["responses"];
    let threads = list(&report["threads"]);
    let sources = list(&report["sources"]);
    let missing = list(&report["missing_children"]);
    let reconciled =
        if report["partial"] != true && responses["total_tokens"] == totals["total_tokens"] {
            display(&responses["total_tokens"])
        } else {
            "unknown".into()
        };
    let uncached = responses["input_tokens"]
        .as_u64()
        .zip(responses["cached_input_tokens"].as_u64())
        .and_then(|(i, c)| i.checked_sub(c))
        .map_or_else(|| "unknown".into(), |v| v.to_string());
    let versions: BTreeSet<_> = threads
        .iter()
        .filter_map(|r| r["cli_version"].as_str())
        .collect();
    let versions = if versions.is_empty() {
        "unknown".into()
    } else {
        versions.into_iter().collect::<Vec<_>>().join(", ")
    };
    let sum = |key: &str| {
        threads
            .iter()
            .filter_map(|r| r[key].as_u64())
            .map(u128::from)
            .sum::<u128>()
    };
    let mut lines: Vec<String> = [
        "# Subscription usage from existing local rollouts",
        "",
        "Observed native JSONL accounting only. Token counts are not weekly quota",
        "percentages, billing, or proof that a later task caused a reset-window change.",
        "Reasoning output is included in output. Cumulative thread snapshots are not",
        "summed with per-response deltas. Raw logs, identities and input hashes stay",
        "in host-private evidence.",
        "",
        "| Category | Count / tokens | Notes |",
        "| --- | ---: | --- |",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let mut line = |category: &str, value: String, notes: &str| {
        lines.push(format!("| {category} | {value} | {notes} |"))
    };
    line("CLI versions", versions, "Native session_meta");
    line(
        "Threads supplied",
        display(&totals["thread_count"]),
        "Explicit inputs only",
    );
    line(
        "Distinct response IDs",
        display(&responses["response_count"]),
        "Counted once",
    );
    line(
        "Thread input",
        display(&totals["input_tokens"]),
        "Last cumulative snapshot per thread",
    );
    line(
        "Cached input",
        display(&totals["cached_input_tokens"]),
        "Included in input",
    );
    line(
        "Thread output",
        display(&totals["output_tokens"]),
        "Includes reasoning",
    );
    line(
        "Reasoning output",
        display(&totals["reasoning_output_tokens"]),
        "Not added again",
    );
    line(
        "Reconciled total",
        reconciled,
        "Unknown when coverage is partial or the series disagree; not a quota share",
    );
    line(
        "Cumulative snapshot sum",
        display(&totals["total_tokens"]),
        "Last snapshot per supplied thread; inherited history can overlap",
    );
    line(
        "Per-response delta sum",
        display(&responses["total_tokens"]),
        "Deduplicated stable response IDs; partial observed series, not added to cumulative snapshots",
    );
    line(
        "Response input / cached / uncached",
        format!(
            "{} / {} / {uncached}",
            display(&responses["input_tokens"]),
            display(&responses["cached_input_tokens"])
        ),
        "Cached input is included in input",
    );
    line(
        "Automatic context occurrences / chars",
        format!("{} / {}", sum("hook_messages"), sum("hook_chars")),
        "Recognized hook/subagent/internal-context markers; inherited or repeated occurrences are not deduplicated findings",
    );
    line(
        "Ordinary turns started",
        sum("task_started").to_string(),
        "Not counted as continuation",
    );
    line(
        "Commentary messages",
        sum("commentary_messages").to_string(),
        "Ordinary status; not continuation",
    );
    line(
        "Continuation notices",
        sum("continuation_notices").to_string(),
        "Interrupt/timeout/resume trigger text",
    );
    line(
        "Actual continuations",
        sum("actual_continuations").to_string(),
        "Distinct resumptions correlated with the next later turn before an ordinary user request; heuristic, not causal proof",
    );
    line(
        "Missing children",
        missing.len().to_string(),
        "Spawn recorded, rollout not supplied",
    );
    line(
        "Compacted windows",
        sum("compacted_windows").to_string(),
        "Compaction envelopes are not extra responses; cumulative inheritance may overlap",
    );
    lines.extend(
        [
            "",
            "## Model / effort / project",
            "",
            "| Role | Model | Provider | Effort | Project | Elapsed s | Responses | Partial |",
            "| --- | --- | --- | --- | --- | ---: | ---: | --- |",
        ]
        .map(str::to_owned),
    );
    #[derive(Default)]
    struct Group {
        threads: usize,
        elapsed: u128,
        known: bool,
        responses: u128,
        partial: bool,
    }
    let mut groups: BTreeMap<[String; 5], Group> = BTreeMap::new();
    for row in threads {
        let key = [
            if row["parent_id"].is_string() {
                "child"
            } else {
                "root"
            }
            .to_owned(),
            row["model"].as_str().unwrap_or("unknown").to_owned(),
            row["provider"].as_str().unwrap_or("unknown").to_owned(),
            row["reasoning"].as_str().unwrap_or("unknown").to_owned(),
            public_project(&row["project"]),
        ];
        let group = groups.entry(key).or_default();
        group.threads += 1;
        if let Some(elapsed) = row["elapsed_seconds"].as_u64() {
            group.elapsed += u128::from(elapsed);
            group.known = true;
        }
        group.responses += u128::from(row["response_count"].as_u64().unwrap_or(0));
        group.partial |= row["partial"] == true;
    }
    for ([role, model, provider, effort, project], group) in groups {
        let elapsed = if group.known {
            group.elapsed.to_string()
        } else {
            "unknown".into()
        };
        lines.push(format!(
            "| {role} x{} | {model} | {provider} | {effort} | {project} | {elapsed} | {} | {} |",
            group.threads,
            group.responses,
            if group.partial { "yes" } else { "no" }
        ));
    }
    if !missing.is_empty() {
        lines.push(
            "| missing-child | unknown | unknown | unknown | unknown | unknown | 0 | yes |".into(),
        );
    }
    lines.extend(["","## Limitations","",
        "- Partial records stay explicit; missing usage is null, not zero.",
        "- Concurrent parent/child elapsed time is not attributed to one task.",
        "- Quota snapshots from different reset windows are incomparable.",
        "- Cumulative snapshot sums and deduplicated response deltas are separate series. Their disagreement is unresolved; neither is a reconciled complete total when coverage is partial.",
        "- Automatic-context counts are recognized marker occurrences, not unique findings.",
        "- Actual continuations are a timestamp heuristic: an intervening ordinary user request breaks correlation, and two triggers sharing one resumed turn count once.",
        "- Source transcripts, credentials and absolute paths are omitted."].map(str::to_owned));
    lines.push(format!(
        "- Private source count: {} hashed inputs retained only in host evidence.",
        sources.len()
    ));
    lines.push(String::new());
    let codes: BTreeSet<_> = list(&report["warnings"])
        .iter()
        .filter_map(|v| v["code"].as_str())
        .collect();
    if !codes.is_empty() {
        lines.push(format!(
            "Warning codes: {}.",
            codes.into_iter().collect::<Vec<_>>().join(", ")
        ));
        lines.push(String::new());
    }
    lines.join("\n")
}
