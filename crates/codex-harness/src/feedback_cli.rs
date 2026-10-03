//! Installed `codex-harness feedback`: deterministic, model-free board
//! bookkeeping over the bd-backed algorithms in `harness_core`.
//!
//! Explicit operations over one durable board: `record` writes one bounded
//! observation, `list`, `ledger` and `candidates` only read, `triage` applies
//! caller-selected semantic decisions in one configured batch, and `promote`
//! performs one explicit recorded promotion. The `hypothesis-*` verbs own the
//! self-improvement hypothesis cards (admission with prior-result search,
//! experiment trials with their nonblocking relationship, implementation
//! references and evidence-bound decisions), and the `removal-*` verbs record
//! reviewable removal proposals and the user's scoped approval, refusal or
//! withdrawal separately from any benefit outcome. `ledger` also parses the
//! item's `pacing-observation`, `pacing-decision`, `pacing-revoke` and
//! `benefit-gate` (v1 and v2) board comments and reports the scoped account
//! view, the applicable pacing decisions and the recorded benefit-gate
//! decision separated from its comparison consistency and evidence
//! limitations (recorded evidence only; never an independent rerun).
//! Similarity, merging and consequence
//! stay caller decisions. Thresholds and batch size come from the
//! owning orchestration configuration: the installed kit checkout recorded by
//! the normal CODEX_HOME installation, else the project's own checkout, else
//! the kit defaults; `--source` names an explicit override and every verb
//! prints the configuration source it used. No operation calls a model, and
//! read-only verbs never mutate the board.

use harness_core::board_feedback::{
    self, BatchFailure, BoundedFeedback, FeedbackDraft, KitConcern, ObservationKind,
    PartialRoutedReport, PromotionEvidence, PromotionOutcome, PromotionRoute, ReporterKind,
    RouteTarget, RoutedAction, VoteDecision, VoteLedger,
};
use harness_core::board_hypothesis;
use harness_core::{benefit_gate, pacing, scoped_observations};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

const USAGE: &str = "codex-harness feedback record --project DIRECTORY --observation TEXT --scope TEXT --reporter ID --episode ID --kind lead|executor|diagnostic --parent ID [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback list --project DIRECTORY [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback ledger --project DIRECTORY --item ID [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback triage --project DIRECTORY --decisions FILE [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback candidates --project DIRECTORY [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback promote --project DIRECTORY --item ID [--route backlog-task|openspec-change|kit-backlog|default] [--openspec-change NAME] [--kit-project DIRECTORY --summary TEXT --scope TEXT] [--override-consequence TEXT --override-reason TEXT] [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback hypothesis-admit --project DIRECTORY --mechanism TOKEN --conditions TOKEN --observation REF --predicted TEXT --counterexample TEXT --acceptance TEXT --spec REF --basis REF [--fresh-basis REF] [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback hypothesis-search --project DIRECTORY [--mechanism TOKEN] [--conditions TOKEN] [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback hypothesis-trial --project DIRECTORY --item ID --experiment ID --role candidate|workload --counterpart ID [--evidence REF] [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback hypothesis-implement --project DIRECTORY --item ID --role candidate|workload --branch NAME --base REVISION --revision REVISION --worktree LOCATOR [--runtime ID] [--baseline-runtime ID] [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback hypothesis-decision --project DIRECTORY --item ID --experiment ID --outcome adopt|reject|inconclusive --quality unchanged|improved|regressed|unmeasurable --matched N --tolerance-percent VALUE --baseline-seconds VALUE --candidate-seconds VALUE --baseline-arm NAME --candidate-arm NAME --accounting TERMS --baseline-revision REVISION --candidate-revision REVISION --acceptance REF --coverage TERMS --scope TOKEN --reason TOKEN [--detail TEXT] [--close yes] [--defer UNTIL] [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback removal-propose --project DIRECTORY --item ID --proposal REF --target TOKEN --evidence REF --loss TOKEN [--preview REF] [--detail TEXT] [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback removal-decide --project DIRECTORY --item ID --decision approve|refuse|withdraw --proposal REF --target TOKEN [--actions experiment,integration,publication] --loss TOKEN [--basis REF] [--detail TEXT] [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback removal-check --project DIRECTORY --item ID --proposal REF --target TOKEN --action experiment|integration|publication [--bd FILE] [--source DIRECTORY]\nRecords, triages, inspects and promotes board feedback through the consuming project's bd board. Thresholds and the triage batch size come from --source/global/orchestration.toml when --source is given, else from the installed kit checkout recorded by CODEX_HOME/harness/installation.json, else from the project's own global/orchestration.toml, else from the kit defaults; every verb prints the configuration source it used. The triage decisions file is strict versioned JSON: {\"schema\": 1, \"decisions\": [{\"feedback\": \"ID\", \"kind\": \"process\", \"merge_into\": \"ID or null\"}]}. `ledger` also parses the item's `pacing-observation`, `pacing-decision`, `pacing-revoke` and `benefit-gate` (v1 and v2) records from the same native bd comments and reports the scoped account view (missing or unknown telemetry stays unknown), the applicable pacing decisions and the item's recorded benefit-gate decision with its comparison consistency, its recorded evidence binding (experiment, exact revisions, acceptance evidence, metric coverage, scope and reason) and its limitations (the record is read back; it is not rerun or independently certified here). Semantic grouping and consequence are caller decisions: this command adds no similarity, no model, no tracker and no implementation authority. An openspec-change promotion validates the intended change directory the OpenSpec workflow created (`openspec new change NAME`) and records its reference as the promotion target; the harness never writes into openspec/ and rerunning preserves an existing draft while it reconciles the board.\nThe hypothesis verbs keep one durable `task` labeled hypothesis per hypothesis: admission searches open, closed and deferred cards first, reuses the prior same-condition conclusion and requires a recorded fresh basis before reconsideration, so a repeated trial creates no duplicate card; `hypothesis-trial` records the explicit candidate/workload relationship as a nonblocking `related` edge plus its experiment reference; `hypothesis-implement` records the candidate branch/base/revision/worktree and prepared runtime identities without adopting or closing anything; `hypothesis-decision` publishes the evidence-bound `benefit-gate v2` record (experiment, exact revisions, acceptance evidence, metric coverage, scope and reason) idempotently and refuses a decision its own comparison cannot support, then optionally closes or defers the card. The removal verbs keep the user's decision separate from measured benefit: `removal-propose` records each reviewed proposal version (any changed reviewed field, including the bounded prose detail, is appended as the new current version), `removal-decide` records the user's approve/refuse/withdraw bound to that exact reviewed content (an approval must name its reviewed loss, which must equal the recorded proposal's loss; the recorded `reviewed=` digest covers every reviewed field), and `removal-check` resolves the latest decision for one exact proposal/target/action (exit 0 only when authorized): an unreadable, incomplete-proposal or changed-proposal newer record never falls back to an older approval, a refusal or withdrawal authorizes nothing, a proposal version recorded after the decision needs a fresh decision before any further removal effect, and no benefit verdict is accepted as consent. No verb creates votes, another tracker or a model call.";

/// Bound on the caller-supplied triage decision document.
const MAX_DECISIONS_BYTES: u64 = 256 * 1024;
/// Bound on one batch document; larger sets are split by the caller.
const MAX_DECISIONS: usize = 512;
/// Bound on the installation record read for the installed kit source.
const MAX_INSTALLATION_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionsDocument {
    schema: u32,
    decisions: Vec<Decision>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    feedback: String,
    kind: String,
    merge_into: Option<String>,
}

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args.first().is_some_and(|arg| arg == "--help") {
        println!("{USAGE}");
        return Ok(0);
    }
    match args.first().and_then(|arg| arg.to_str()) {
        Some("record") => record(&args[1..]),
        Some("list") => list(&args[1..]),
        Some("ledger") => ledger(&args[1..]),
        Some("triage") => triage(&args[1..]),
        Some("candidates") => candidates(&args[1..]),
        Some("promote") => promote(&args[1..]),
        Some("hypothesis-admit") => hypothesis_admit(&args[1..]),
        Some("hypothesis-search") => hypothesis_search(&args[1..]),
        Some("hypothesis-trial") => hypothesis_trial(&args[1..]),
        Some("hypothesis-implement") => hypothesis_implement(&args[1..]),
        Some("hypothesis-decision") => hypothesis_decision(&args[1..]),
        Some("removal-propose") => removal_propose(&args[1..]),
        Some("removal-decide") => removal_decide(&args[1..]),
        Some("removal-check") => removal_check(&args[1..]),
        _ => Err(invalid(
            "invalid feedback options; use codex-harness feedback --help",
        )),
    }
}

/// One resolved board context: the bd executable, the project board, and the
/// limits the operation must honor.
struct Board {
    bd: PathBuf,
    project: PathBuf,
    batch_limit: usize,
    vote_threshold: u32,
    incubator_cap: usize,
    /// Where the limits came from, printed so the caller never has to guess.
    limits: String,
}

fn resolve_board(options: &Options) -> io::Result<Board> {
    let project = PathBuf::from(options.required("--project")?);
    if !project.is_dir() {
        return Err(invalid(&format!(
            "project directory {} is missing; --project names the bd board root",
            project.display()
        )));
    }
    let bd = match options.get("--bd") {
        Some(path) => PathBuf::from(path),
        None => discover_bd()?,
    };
    if !bd.is_file() {
        return Err(board_cli_unavailable(&format!(
            "bd executable is missing at {}; connect the board component (install --board-only) or pass --bd FILE",
            bd.display()
        )));
    }
    let limits = resolve_limits(options, &project)?;
    let (batch_limit, vote_threshold, incubator_cap) =
        match harness_core::orchestration_config::load(&limits.checkout) {
            Ok(config) => (
                config.feedback_batch_limit as usize,
                config.vote_threshold,
                config.incubator_size_cap as usize,
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (
                board_feedback::DEFAULT_FEEDBACK_BATCH_LIMIT,
                board_feedback::DEFAULT_VOTE_THRESHOLD,
                board_feedback::DEFAULT_INCUBATOR_SIZE_CAP,
            ),
            Err(error) => {
                return Err(invalid(&format!(
                    "orchestration configuration {} is unusable: {error}",
                    limits.checkout.join("global/orchestration.toml").display()
                )));
            }
        };
    Ok(Board {
        bd,
        project,
        batch_limit,
        vote_threshold,
        incubator_cap,
        limits: limits.account,
    })
}

/// The owning orchestration configuration and the account of how it was
/// found. `--source` is the explicit override; otherwise the installed kit
/// checkout the normal installation recorded under CODEX_HOME owns the
/// thresholds, then the project's own checkout (a source checkout carrying the
/// kit configuration), then the kit defaults.
fn resolve_limits(options: &Options, project: &Path) -> io::Result<Limits> {
    let config_of = |checkout: &Path| checkout.join("global/orchestration.toml");
    if let Some(source) = options.get("--source") {
        let checkout = PathBuf::from(source);
        if !checkout.is_dir() {
            return Err(invalid(&format!(
                "source checkout {} is missing; --source names the checkout whose global/orchestration.toml supplies the limits",
                checkout.display()
            )));
        }
        let config = config_of(&checkout);
        let account = if config.is_file() {
            format!("configured({})", config.display())
        } else {
            format!(
                "defaults({} is absent; --source named no configuration)",
                config.display()
            )
        };
        return Ok(Limits { checkout, account });
    }
    let installed = installed_kit_source();
    if let Some(kit) = &installed {
        let config = config_of(kit);
        if config.is_file() {
            return Ok(Limits {
                checkout: kit.clone(),
                account: format!("configured(installed kit {})", config.display()),
            });
        }
    }
    let config = config_of(project);
    if config.is_file() {
        return Ok(Limits {
            checkout: project.to_path_buf(),
            account: format!("configured(project {})", config.display()),
        });
    }
    let kit_note = match &installed {
        Some(kit) => format!("{} is absent too", config_of(kit).display()),
        None => "no readable installation record was found".to_owned(),
    };
    Ok(Limits {
        checkout: project.to_path_buf(),
        account: format!("defaults({} is absent and {kit_note})", config.display()),
    })
}

/// The kit source root recorded by the normal installation. The CODEX_HOME
/// location comes from the launcher's own resolution (`CODEX_HOME`, else
/// `USERPROFILE\.codex`), so an unset variable still finds installed custom
/// limits; the current schema keeps the root at `settings.sourceRoot`. Reading
/// is bounded and executes nothing the record names; an absent, unreadable or
/// relocated record simply means no installed kit source is available.
fn installed_kit_source() -> Option<PathBuf> {
    let codex_home = harness_core::native_launcher::codex_home().ok()?;
    let path = codex_home.join("harness/installation.json");
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_INSTALLATION_BYTES {
        return None;
    }
    let bytes = fs::read(&path).ok()?;
    let record: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    // Current metadata keeps every owner inside `settings`; the legacy
    // PowerShell import schema kept `sourceRoot` at the top level. Only these
    // two documented locations are read: discovery is read-only and validates
    // nothing beyond the path it needs.
    let root = record
        .get("settings")
        .and_then(|settings| settings.get("sourceRoot"))
        .or_else(|| record.get("sourceRoot"))
        .and_then(|value| value.as_str())?;
    let root = PathBuf::from(root);
    root.is_dir().then_some(root)
}

struct Limits {
    checkout: PathBuf,
    account: String,
}

fn board_cli_unavailable(detail: &str) -> io::Error {
    harness_core::board_cli::unavailable(detail)
}

fn discover_bd() -> io::Result<PathBuf> {
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        let candidate = PathBuf::from(home).join("harness/bin").join(bd_name());
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let lower = dir.to_string_lossy().to_ascii_lowercase();
            if lower.ends_with(r"\windowsapps") || lower.contains(r"\windowsapps\") {
                continue;
            }
            let candidate = dir.join(bd_name());
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(board_cli_unavailable(
        "bd is not at CODEX_HOME/harness/bin and not on PATH; connect the board component (install --board-only) or pass --bd FILE",
    ))
}

fn bd_name() -> &'static str {
    if cfg!(windows) { "bd.exe" } else { "bd" }
}

fn record(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&[
        "--project",
        "--bd",
        "--source",
        "--observation",
        "--scope",
        "--reporter",
        "--episode",
        "--kind",
        "--parent",
    ])?;
    let board = resolve_board(&options)?;
    let draft = FeedbackDraft {
        observation: options.required("--observation")?.to_owned(),
        scope: options.required("--scope")?.to_owned(),
        reporter: options.required("--reporter")?.to_owned(),
        episode: options.required("--episode")?.to_owned(),
        kind: ReporterKind::parse(options.required("--kind")?)?,
        parent_id: options.required("--parent")?.to_owned(),
    };
    let bounded = BoundedFeedback::try_from_draft(draft)?;
    let id = board_feedback::record_feedback(&board.bd, &board.project, &bounded)?;
    println!(
        "recorded feedback {id} reporter={} episode={} kind={} parent={} limits={}",
        bounded.reporter,
        bounded.episode,
        bounded.kind.as_str(),
        bounded.parent_id,
        board.limits
    );
    Ok(0)
}

fn list(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&["--project", "--bd", "--source"])?;
    let board = resolve_board(&options)?;
    let rows = board_feedback::list_feedback(&board.bd, &board.project)?;
    println!(
        "feedback: {} open batch_limit={} limits={}",
        rows.len(),
        board.batch_limit,
        board.limits
    );
    for row in &rows {
        println!(
            "{} reporter={} episode={} kind={} scope={} observation={}",
            row.id,
            row.feedback.reporter,
            row.feedback.episode,
            row.feedback.kind.as_str(),
            row.feedback.scope,
            display_text(&row.feedback.observation)
        );
    }
    Ok(0)
}

fn ledger(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&["--project", "--bd", "--source", "--item"])?;
    let board = resolve_board(&options)?;
    let item = options.required("--item")?;
    let ledger = board_feedback::inspect_ledger(&board.bd, &board.project, item)?;
    println!(
        "ledger {item} votes={} counted={} merges={} routes={} promotions={} limits={}",
        ledger.votes.len(),
        ledger.counted(),
        ledger.merges.len(),
        ledger.routes.len(),
        ledger.promotions.len(),
        board.limits
    );
    for vote in &ledger.votes {
        println!(
            "vote episode={} reporter={} kind={} counted={} reason={}",
            vote.episode,
            vote.reporter,
            vote.kind.as_str(),
            vote.counted,
            vote.reason.as_str()
        );
    }
    for merge in &ledger.merges {
        println!("merge from={} into={}", merge.from, merge.into);
    }
    for route in &ledger.routes {
        println!(
            "route kind={} target={} item={}",
            route.kind.as_str(),
            route.target.as_str(),
            route.item
        );
    }
    for promotion in &ledger.promotions {
        println!(
            "promotion route={} basis={} counted={} target={}",
            promotion.route.as_str(),
            if promotion.override_used {
                "override"
            } else {
                "votes"
            },
            promotion.counted,
            promotion.target.as_deref().unwrap_or("none")
        );
    }
    // The same native comments carry the pacing and benefit-gate records; the
    // ledger reports them instead of leaving a second tracker to be invented.
    let comments = board_feedback::list_comments(&board.bd, &board.project, item)?;
    let now = epoch_seconds();
    let observations = scoped_observations::parse_observation_comments(&comments);
    if observations.is_empty() {
        println!("pacing observations: none recorded; telemetry unknown");
    } else {
        let mut scopes: Vec<&str> = observations
            .iter()
            .map(|observation| observation.scope.as_str())
            .collect();
        scopes.sort_unstable();
        scopes.dedup();
        for scope in scopes {
            let view = scoped_observations::account_view(&observations, scope, now);
            println!("pacing observation {}", view.describe());
        }
    }
    let pacing_records = pacing::parse_pacing_comments(&comments);
    let applicable = pacing::applicable(&pacing_records, now);
    println!(
        "pacing decisions: {} applicable of {} recorded; {} revoked",
        applicable.len(),
        pacing_records.decisions.len(),
        pacing_records.revoked.len()
    );
    for decision in applicable {
        println!("pacing decision {}", decision.to_comment());
    }
    let gates = benefit_gate::parse_gate_comments(&comments);
    match benefit_gate::assess(&gates, item) {
        None => println!("benefit gate {item}: no comparison recorded; unadopted"),
        Some(assessment) => {
            let latest = assessment.latest;
            println!(
                "benefit gate {item}: recorded outcome={} quality={} across {} attributable record(s)",
                latest.outcome.as_deref().unwrap_or("absent"),
                latest.quality.as_deref().unwrap_or("absent"),
                assessment.recorded
            );
            let (supported, phrase, status) = match assessment.verdict {
                benefit_gate::Verdict::Consistent => (
                    "yes",
                    "recorded comparison consistent within its declared tolerance",
                    "adopted",
                ),
                benefit_gate::Verdict::Unsupported => (
                    "no",
                    "recorded comparison cannot support an adoption",
                    "unadopted",
                ),
                benefit_gate::Verdict::NonAdoption => (
                    "no",
                    "the latest record is not an adoption decision",
                    "unadopted",
                ),
                benefit_gate::Verdict::Unreadable => (
                    "no",
                    "the latest attributable record states no readable decision",
                    "unadopted",
                ),
            };
            println!("benefit gate {item}: supported={supported} ({phrase}) default={status}");
            let binding = binding_text(latest);
            if !binding.is_empty() {
                println!("benefit gate {item}: binding: {binding}");
            }
            if assessment.limitations.is_empty() {
                println!("benefit gate {item}: limitations: none");
            } else {
                let reasons: Vec<&str> = assessment
                    .limitations
                    .iter()
                    .map(|limitation| limitation.as_str())
                    .collect();
                println!("benefit gate {item}: limitations: {}", reasons.join("; "));
            }
            println!(
                "benefit gate {item}: evidence: the recorded board comment only; the comparison has not been rerun or independently certified here"
            );
        }
    }
    for record in &gates {
        let binding = binding_text(record);
        println!(
            "benefit-gate record item={} outcome={} quality={}{}{}",
            record.item,
            record.outcome.as_deref().unwrap_or("absent"),
            record.quality.as_deref().unwrap_or("absent"),
            if binding.is_empty() { "" } else { " " },
            binding
        );
    }
    Ok(0)
}

/// The recorded decision binding - experiment, exact revisions, acceptance
/// evidence, metric coverage, scope and reason - as one space-separated field
/// list. Fields absent from the record stay absent: the summary reports what
/// was recorded and never completes a missing binding with an invented value.
/// The bounded prose detail is not read back, so the summary carries evidence
/// references rather than raw traces.
fn binding_text(record: &benefit_gate::GateRecord) -> String {
    [
        ("experiment", record.experiment.as_deref()),
        ("revisions", record.revisions.as_deref()),
        ("acceptance", record.acceptance.as_deref()),
        ("coverage", record.coverage.as_deref()),
        ("scope", record.scope.as_deref()),
        ("reason", record.reason.as_deref()),
    ]
    .into_iter()
    .filter_map(|(name, value)| {
        value
            .filter(|value| !value.is_empty())
            .map(|value| format!("{name}={value}"))
    })
    .collect::<Vec<_>>()
    .join(" ")
}

fn candidates(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&["--project", "--bd", "--source"])?;
    let board = resolve_board(&options)?;
    let candidates =
        board_feedback::promotion_candidates(&board.bd, &board.project, board.vote_threshold)?;
    println!(
        "promotion candidates: {} at threshold={} limits={}",
        candidates.len(),
        board.vote_threshold,
        board.limits
    );
    for candidate in &candidates {
        println!(
            "candidate {} counted={} threshold={} kinds={}",
            candidate.item_id,
            candidate.counted,
            board.vote_threshold,
            kinds_text(&candidate.kinds)
        );
    }
    if let Some(size) =
        board_feedback::incubator_over_cap(&board.bd, &board.project, board.incubator_cap)?
    {
        println!(
            "incubator above cap: size={size} cap={}; the lead sweeps at a safe boundary",
            board.incubator_cap
        );
    }
    Ok(0)
}

fn triage(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&["--project", "--bd", "--source", "--decisions"])?;
    let board = resolve_board(&options)?;
    let decisions_path = PathBuf::from(options.required("--decisions")?);
    let actions = load_decisions(&decisions_path)?;
    let report = board_feedback::apply_routed_triage_recovering(
        &board.bd,
        &board.project,
        &actions,
        board.batch_limit,
    )?;
    for outcome in &report.applied {
        println!(
            "applied feedback {} route={} target={} vote={}",
            outcome.feedback_id,
            route_text(outcome.route),
            outcome.target_id,
            vote_text(&outcome.vote)
        );
    }
    if report.deferred > 0 {
        println!(
            "deferred {} decisions beyond feedback_batch_limit={}: rerun with the remaining decisions",
            report.deferred, board.batch_limit
        );
    }
    report_outcome(&report)
}

/// Prints the partial-failure report and returns the recovery exit code: the
/// applied prefix stays visible, the failing operation is named, and the
/// caller knows that rerunning the same decisions continues instead of
/// duplicating counted votes or merges.
fn report_outcome(report: &PartialRoutedReport) -> io::Result<i32> {
    match &report.failure {
        None => {
            println!("triage complete: applied={}", report.applied.len());
            Ok(0)
        }
        Some(BatchFailure { operation, error }) => {
            eprintln!("failed: {operation}: {error}");
            eprintln!(
                "applied {} decisions before the failure; rerun the same decisions to continue: recorded votes stay counted once and an applied merge or route is not repeated",
                report.applied.len()
            );
            Ok(1)
        }
    }
}

fn promote(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&[
        "--project",
        "--bd",
        "--source",
        "--item",
        "--route",
        "--openspec-change",
        "--kit-project",
        "--summary",
        "--scope",
        "--override-consequence",
        "--override-reason",
    ])?;
    let board = resolve_board(&options)?;
    let item = options.required("--item")?;
    let route = match options.get("--route") {
        None | Some("default") => None,
        Some(value) => Some(PromotionRoute::parse(value).ok_or_else(|| {
            invalid(&format!(
                "unknown promotion route {value}; use backlog-task, openspec-change, kit-backlog or default"
            ))
        })?),
    };
    let kit_arguments = ["--kit-project", "--summary", "--scope"]
        .iter()
        .filter(|key| options.get(key).is_some())
        .count();
    match route {
        Some(PromotionRoute::KitBacklog) => {
            if kit_arguments != 3 {
                return Err(invalid(
                    "a kit-backlog promotion requires --kit-project DIRECTORY --summary TEXT --scope TEXT with kit-level wording only",
                ));
            }
        }
        // A derived route may resolve to the kit backlog, so give the wording
        // all three parts or none; a partial set is always a mistake.
        None => {
            if kit_arguments != 0 && kit_arguments != 3 {
                return Err(invalid(
                    "kit-level wording needs --kit-project DIRECTORY, --summary TEXT and --scope TEXT together",
                ));
            }
        }
        Some(_) => {
            if kit_arguments > 0 {
                return Err(invalid(
                    "--kit-project, --summary and --scope apply only to a kit-backlog promotion",
                ));
            }
        }
    }
    let consequence = options.get("--override-consequence");
    let reason = options.get("--override-reason");
    let evidence = match (consequence, reason) {
        (Some(consequence), Some(reason)) => PromotionEvidence::ConsequenceOverride {
            consequence: consequence.to_owned(),
            reason: reason.to_owned(),
        },
        (None, None) => PromotionEvidence::Votes {
            threshold: board.vote_threshold,
        },
        _ => {
            return Err(invalid(
                "--override-consequence and --override-reason are both required for a consequence override",
            ));
        }
    };
    let route = match route {
        Some(route) => route,
        None => default_route(&board, item)?,
    };
    let openspec_change = options
        .get("--openspec-change")
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if openspec_change.is_some() && route != PromotionRoute::OpenSpecChange {
        return Err(invalid(
            "--openspec-change applies only to a promotion whose route is openspec-change",
        ));
    }
    let outcome = run_promotion(&board, item, route, &options, &evidence, openspec_change)?;
    if outcome.already_recorded {
        println!(
            "promotion already recorded: {} route={} basis={} counted={} threshold={} target={} limits={} (no promotion record was written; any missing labels are reconciled)",
            outcome.item_id,
            outcome.route.as_str(),
            if outcome.override_used {
                "override"
            } else {
                "votes"
            },
            outcome.counted,
            if outcome.override_used {
                "none".to_owned()
            } else {
                board.vote_threshold.to_string()
            },
            outcome.target.as_deref().unwrap_or("none"),
            board.limits
        );
        return Ok(0);
    }
    println!(
        "promoted {} route={} basis={} counted={} threshold={} target={} limits={}",
        outcome.item_id,
        outcome.route.as_str(),
        if outcome.override_used {
            "override"
        } else {
            "votes"
        },
        outcome.counted,
        if outcome.override_used {
            "none".to_owned()
        } else {
            board.vote_threshold.to_string()
        },
        outcome.target.as_deref().unwrap_or("none"),
        board.limits
    );
    Ok(0)
}

/// The consequence route the recorded classification implies. An unclassified
/// item cannot be promoted by default: the caller either classifies it at
/// triage or states the route explicitly.
fn default_route(board: &Board, item: &str) -> io::Result<PromotionRoute> {
    let ledger = board_feedback::inspect_ledger(&board.bd, &board.project, item)?;
    let ks = routed_kinds(&ledger);
    board_feedback::default_promotion_route(&ks).ok_or_else(|| {
        invalid(&format!(
            "incubator item {item} has no recorded classification; triage it with a kind first or pass --route explicitly"
        ))
    })
}

fn run_promotion(
    board: &Board,
    item: &str,
    route: PromotionRoute,
    options: &Options,
    evidence: &PromotionEvidence,
    openspec_change: Option<&str>,
) -> io::Result<PromotionOutcome> {
    let outcome = match route {
        PromotionRoute::KitBacklog => {
            let kit_project = PathBuf::from(options.required("--kit-project")?);
            let concern = KitConcern {
                summary: options.required("--summary")?.to_owned(),
                scope: options.required("--scope")?.to_owned(),
            };
            board_feedback::promote_kit_concern(
                &board.bd,
                &board.project,
                &kit_project,
                item,
                &concern,
                evidence,
            )
        }
        route => board_feedback::promote_item(
            &board.bd,
            &board.project,
            item,
            route,
            evidence,
            openspec_change,
        ),
    };
    outcome.map_err(|error| {
        invalid(&format!(
            "{error}; rerun the same feedback promote after resolving the cause: a recorded promotion is never repeated and its labels are reconciled"
        ))
    })
}

/// Admits one hypothesis card. The prior-result search covers open, closed
/// and deferred cards with the same mechanism and conditions: an existing
/// conclusion is reused, a fresh basis reconsiders it on the same card, and
/// nothing here creates a duplicate card, a vote or a model call.
fn hypothesis_admit(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&[
        "--project",
        "--bd",
        "--source",
        "--mechanism",
        "--conditions",
        "--observation",
        "--predicted",
        "--counterexample",
        "--acceptance",
        "--spec",
        "--basis",
        "--fresh-basis",
    ])?;
    let board = resolve_board(&options)?;
    let bounded =
        board_hypothesis::BoundedHypothesis::try_from_draft(board_hypothesis::HypothesisDraft {
            mechanism: options.required("--mechanism")?.to_owned(),
            conditions: options.required("--conditions")?.to_owned(),
            observation: options.required("--observation")?.to_owned(),
            predicted: options.required("--predicted")?.to_owned(),
            counterexample: options.required("--counterexample")?.to_owned(),
            acceptance: options.required("--acceptance")?.to_owned(),
            spec: options.required("--spec")?.to_owned(),
            basis: options.required("--basis")?.to_owned(),
        })?;
    match board_hypothesis::admit_hypothesis(
        &board.bd,
        &board.project,
        &bounded,
        options.get("--fresh-basis"),
    )? {
        board_hypothesis::Admission::Created { id } => println!(
            "hypothesis {id} created mechanism={} conditions={} basis={} limits={}",
            bounded.mechanism, bounded.conditions, bounded.basis, board.limits
        ),
        board_hypothesis::Admission::Existing { id, status } => println!(
            "hypothesis {id} existing status={status} mechanism={} conditions={} (a matching card already owns this hypothesis; no duplicate card was created) limits={}",
            bounded.mechanism, bounded.conditions, board.limits
        ),
        board_hypothesis::Admission::ReusedRejection {
            id,
            experiment,
            reason,
            basis,
        } => println!(
            "hypothesis {id} reused conclusion=reject experiment={} reason={} basis={} (the same-condition rejection is reused; a fresh evidential basis is required to reconsider) limits={}",
            experiment.as_deref().unwrap_or("none"),
            reason.as_deref().unwrap_or("none"),
            basis.as_deref().unwrap_or("none"),
            board.limits
        ),
        board_hypothesis::Admission::ReusedInconclusive {
            id,
            experiment,
            reason,
            basis,
        } => println!(
            "hypothesis {id} reused conclusion=inconclusive experiment={} reason={} basis={} (the missing observation is already recorded; a fresh evidential basis is required to reconsider) limits={}",
            experiment.as_deref().unwrap_or("none"),
            reason.as_deref().unwrap_or("none"),
            basis.as_deref().unwrap_or("none"),
            board.limits
        ),
        board_hypothesis::Admission::Reconsidered {
            id,
            basis,
            prior_outcome,
            prior_experiment,
        } => println!(
            "hypothesis {id} reconsidered prior_conclusion={prior_outcome} prior_experiment={} basis={basis} (the earlier result is preserved on the same card) limits={}",
            prior_experiment.as_deref().unwrap_or("none"),
            board.limits
        ),
    }
    Ok(0)
}

/// Lists hypothesis cards across every status with their latest recorded
/// decision; `--mechanism`/`--conditions` restrict the search exactly.
fn hypothesis_search(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&[
        "--project",
        "--bd",
        "--source",
        "--mechanism",
        "--conditions",
    ])?;
    let board = resolve_board(&options)?;
    let filter = board_hypothesis::SearchFilter {
        mechanism: options.get("--mechanism").map(str::to_owned),
        conditions: options.get("--conditions").map(str::to_owned),
    };
    let cards = board_hypothesis::search_hypotheses(&board.bd, &board.project, &filter)?;
    println!(
        "hypotheses: {} matching mechanism={} conditions={} limits={}",
        cards.len(),
        options.get("--mechanism").unwrap_or("any"),
        options.get("--conditions").unwrap_or("any"),
        board.limits
    );
    for card in &cards {
        println!(
            "hypothesis {} status={} mechanism={} conditions={} decisions={} trials={} implementations={} reconsiderations={} spec={} basis={}",
            card.id,
            card.status,
            card.mechanism.as_deref().unwrap_or("unknown"),
            card.conditions.as_deref().unwrap_or("unknown"),
            card.decisions,
            card.trials,
            card.implementations,
            card.reconsiderations,
            card.spec.as_deref().unwrap_or("none"),
            card.basis.as_deref().unwrap_or("none")
        );
        if let Some(decision) = &card.latest_decision {
            println!(
                "hypothesis {} latest={} experiment={} reason={}",
                card.id,
                decision.outcome.as_deref().unwrap_or("unreadable"),
                decision.experiment.as_deref().unwrap_or("none"),
                decision.reason.as_deref().unwrap_or("none")
            );
        }
    }
    Ok(0)
}

/// Records one experiment trial and its nonblocking candidate/workload
/// relationship. The identical trial is never recorded twice.
fn hypothesis_trial(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&[
        "--project",
        "--bd",
        "--source",
        "--item",
        "--experiment",
        "--role",
        "--counterpart",
        "--evidence",
    ])?;
    let board = resolve_board(&options)?;
    let item = options.required("--item")?;
    let role = board_hypothesis::HypothesisRole::parse(options.required("--role")?)
        .ok_or_else(|| invalid("--role is candidate or workload"))?;
    let trial = board_hypothesis::BoundedTrial::try_from_draft(board_hypothesis::TrialDraft {
        experiment: options.required("--experiment")?.to_owned(),
        role,
        counterpart: options.required("--counterpart")?.to_owned(),
        evidence: options.get("--evidence").map(str::to_owned),
    })?;
    let record = board_hypothesis::record_trial(&board.bd, &board.project, item, &trial)?;
    println!(
        "trial hypothesis {} experiment={} role={} counterpart={} comment={} related={} limits={}",
        item,
        record.experiment,
        record.role.as_str(),
        record.counterpart,
        if record.recorded {
            "written"
        } else {
            "already-recorded"
        },
        if record.related {
            "established"
        } else {
            "already-present"
        },
        board.limits
    );
    Ok(0)
}

/// Records one candidate implementation reference (branch, base, revision,
/// worktree and prepared runtime identities) without adopting anything.
fn hypothesis_implement(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&[
        "--project",
        "--bd",
        "--source",
        "--item",
        "--role",
        "--branch",
        "--base",
        "--revision",
        "--worktree",
        "--runtime",
        "--baseline-runtime",
    ])?;
    let board = resolve_board(&options)?;
    let item = options.required("--item")?;
    let role = board_hypothesis::HypothesisRole::parse(options.required("--role")?)
        .ok_or_else(|| invalid("--role is candidate or workload"))?;
    let implementation = board_hypothesis::BoundedImplementation::try_from_draft(
        board_hypothesis::ImplementationDraft {
            role,
            branch: options.required("--branch")?.to_owned(),
            base: options.required("--base")?.to_owned(),
            revision: options.required("--revision")?.to_owned(),
            worktree: options.required("--worktree")?.to_owned(),
            runtime: options.get("--runtime").map(str::to_owned),
            baseline_runtime: options.get("--baseline-runtime").map(str::to_owned),
        },
    )?;
    let record =
        board_hypothesis::record_implementation(&board.bd, &board.project, item, &implementation)?;
    println!(
        "implementation hypothesis {} role={} branch={} revision={} comment={} limits={}",
        item,
        record.role.as_str(),
        record.branch,
        record.revision,
        if record.recorded {
            "written"
        } else {
            "already-recorded"
        },
        board.limits
    );
    Ok(0)
}

/// Publishes one evidence-bound decision on the hypothesis card, optionally
/// closing or deferring the card. The identical decision is never published
/// twice, and a decision its own comparison cannot support is refused.
fn hypothesis_decision(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&[
        "--project",
        "--bd",
        "--source",
        "--item",
        "--experiment",
        "--outcome",
        "--quality",
        "--matched",
        "--tolerance-percent",
        "--baseline-seconds",
        "--candidate-seconds",
        "--baseline-arm",
        "--candidate-arm",
        "--accounting",
        "--baseline-revision",
        "--candidate-revision",
        "--acceptance",
        "--coverage",
        "--scope",
        "--reason",
        "--detail",
        "--close",
        "--defer",
    ])?;
    let board = resolve_board(&options)?;
    let outcome = benefit_gate::DecisionOutcome::parse(options.required("--outcome")?)
        .ok_or_else(|| invalid("--outcome is adopt, reject or inconclusive"))?;
    let quality = benefit_gate::QualityOutcome::parse(options.required("--quality")?)
        .ok_or_else(|| invalid("--quality is unchanged, improved, regressed or unmeasurable"))?;
    let close = match options.get("--close") {
        None | Some("no") => false,
        Some("yes") => true,
        Some(other) => return Err(invalid(&format!("--close takes yes or no, not {other}"))),
    };
    let draft = benefit_gate::DecisionDraft {
        item: options.required("--item")?.to_owned(),
        experiment: options.required("--experiment")?.to_owned(),
        outcome,
        quality,
        matched: count("--matched", options.required("--matched")?)?,
        tolerance_percent: number(
            "--tolerance-percent",
            options.required("--tolerance-percent")?,
        )?,
        baseline_seconds: number(
            "--baseline-seconds",
            options.required("--baseline-seconds")?,
        )?,
        candidate_seconds: number(
            "--candidate-seconds",
            options.required("--candidate-seconds")?,
        )?,
        baseline_arm: options.required("--baseline-arm")?.to_owned(),
        candidate_arm: options.required("--candidate-arm")?.to_owned(),
        accounting: options.required("--accounting")?.to_owned(),
        baseline_revision: options.required("--baseline-revision")?.to_owned(),
        candidate_revision: options.required("--candidate-revision")?.to_owned(),
        acceptance: options.required("--acceptance")?.to_owned(),
        coverage: options.required("--coverage")?.to_owned(),
        scope: options.required("--scope")?.to_owned(),
        reason: options.required("--reason")?.to_owned(),
        detail: options.get("--detail").map(str::to_owned),
    };
    let publication = benefit_gate::publish_decision(&board.bd, &board.project, &draft)?;
    println!(
        "decision hypothesis {} experiment={} outcome={} publication={} limits={}",
        draft.item,
        draft.experiment,
        draft.outcome.as_str(),
        match publication {
            benefit_gate::Publication::Recorded { .. } => "written",
            benefit_gate::Publication::Confirmed { .. } => "already-recorded",
        },
        board.limits
    );
    if close {
        board_hypothesis::close_hypothesis(
            &board.bd,
            &board.project,
            &draft.item,
            draft.outcome.as_str(),
            &draft.reason,
            Some(&draft.experiment),
        )?;
        println!(
            "closed hypothesis {} outcome={} reason={} experiment={}",
            draft.item,
            draft.outcome.as_str(),
            draft.reason,
            draft.experiment
        );
    }
    if let Some(until) = options.get("--defer") {
        board_hypothesis::defer_hypothesis(&board.bd, &board.project, &draft.item, until)?;
        println!("deferred hypothesis {} until={until}", draft.item);
    }
    Ok(0)
}

/// Records one reviewable removal proposal; nothing is applied.
fn removal_propose(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&[
        "--project",
        "--bd",
        "--source",
        "--item",
        "--proposal",
        "--target",
        "--evidence",
        "--loss",
        "--preview",
        "--detail",
    ])?;
    let board = resolve_board(&options)?;
    let item = options.required("--item")?;
    let proposal = board_hypothesis::BoundedRemovalProposal::try_from_draft(
        board_hypothesis::RemovalProposalDraft {
            proposal: options.required("--proposal")?.to_owned(),
            target: options.required("--target")?.to_owned(),
            evidence: options.required("--evidence")?.to_owned(),
            loss: options.required("--loss")?.to_owned(),
            preview: options.get("--preview").map(str::to_owned),
            detail: options.get("--detail").map(str::to_owned),
        },
    )?;
    let record =
        board_hypothesis::record_removal_proposal(&board.bd, &board.project, item, &proposal)?;
    println!(
        "removal proposal {} proposal={} target={} record={} limits={}",
        item,
        record.proposal,
        record.target,
        if record.recorded {
            "written"
        } else {
            "already-recorded"
        },
        board.limits
    );
    Ok(0)
}

/// Records the user's explicit removal decision for a presented proposal.
fn removal_decide(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&[
        "--project",
        "--bd",
        "--source",
        "--item",
        "--decision",
        "--proposal",
        "--target",
        "--actions",
        "--loss",
        "--basis",
        "--detail",
    ])?;
    let board = resolve_board(&options)?;
    let item = options.required("--item")?;
    let decision = board_hypothesis::RemovalDecisionKind::parse(options.required("--decision")?)
        .ok_or_else(|| invalid("--decision is approve, refuse or withdraw"))?;
    let actions = match options.get("--actions") {
        None => Vec::new(),
        Some(list) => list
            .split([',', '+'])
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(|token| {
                board_hypothesis::RemovalAction::parse(token).ok_or_else(|| {
                    invalid(&format!(
                        "unknown removal action {token}; use experiment, integration or publication"
                    ))
                })
            })
            .collect::<io::Result<Vec<_>>>()?,
    };
    let bounded = board_hypothesis::BoundedRemovalDecision::try_from_draft(
        board_hypothesis::RemovalDecisionDraft {
            decision,
            proposal: options.required("--proposal")?.to_owned(),
            target: options.required("--target")?.to_owned(),
            actions,
            loss: options.get("--loss").map(str::to_owned),
            basis: options.get("--basis").map(str::to_owned),
            detail: options.get("--detail").map(str::to_owned),
        },
    )?;
    let recorded =
        board_hypothesis::record_removal_decision(&board.bd, &board.project, item, &bounded)?;
    let actions = if bounded.actions.is_empty() {
        "none".to_owned()
    } else {
        bounded
            .actions
            .iter()
            .map(|action| action.as_str())
            .collect::<Vec<_>>()
            .join("+")
    };
    println!(
        "removal decision {} decision={} proposal={} target={} actions={actions} record={} limits={}",
        item,
        bounded.decision.as_str(),
        bounded.proposal,
        bounded.target,
        if recorded {
            "written"
        } else {
            "already-recorded"
        },
        board.limits
    );
    Ok(0)
}

/// Resolves the latest removal decision for one exact proposal, target and
/// action. Exit 0 means authorized; every other outcome exits 1 with the
/// reason, including an unreadable newer decision that cannot fall back to an
/// older approval.
fn removal_check(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.allowed(&[
        "--project",
        "--bd",
        "--source",
        "--item",
        "--proposal",
        "--target",
        "--action",
    ])?;
    let board = resolve_board(&options)?;
    let item = options.required("--item")?;
    let action = board_hypothesis::RemovalAction::parse(options.required("--action")?)
        .ok_or_else(|| invalid("--action is experiment, integration or publication"))?;
    let request = board_hypothesis::AuthorityRequest {
        proposal: options.required("--proposal")?.to_owned(),
        target: options.required("--target")?.to_owned(),
        action,
    };
    let comments = board_feedback::list_comments(&board.bd, &board.project, item)?;
    let authority = board_hypothesis::removal_authority(&comments, item, &request);
    let describe = |record: &board_hypothesis::RemovalDecisionRecord| {
        format!(
            "proposal={} target={} loss={} evidence={} reviewed={} basis={}",
            record.proposal.as_deref().unwrap_or("absent"),
            record.target.as_deref().unwrap_or("absent"),
            record.loss.as_deref().unwrap_or("absent"),
            record.evidence.as_deref().unwrap_or("absent"),
            record.reviewed.as_deref().unwrap_or("absent"),
            record.basis.as_deref().unwrap_or("none")
        )
    };
    match authority {
        board_hypothesis::RemovalAuthority::Authorized { record } => {
            println!(
                "removal authority {item} action={} result=authorized {} limits={}",
                action.as_str(),
                describe(&record),
                board.limits
            );
            Ok(0)
        }
        board_hypothesis::RemovalAuthority::Refused { record } => {
            println!(
                "removal authority {item} action={} result=refused {}; the user declined this removal - do not repeat the request without a new evidential basis or user instruction",
                action.as_str(),
                describe(&record)
            );
            Ok(1)
        }
        board_hypothesis::RemovalAuthority::Withdrawn { record } => {
            println!(
                "removal authority {item} action={} result=withdrawn {}; the approval was withdrawn",
                action.as_str(),
                describe(&record)
            );
            Ok(1)
        }
        board_hypothesis::RemovalAuthority::Missing => {
            println!(
                "removal authority {item} action={} result=missing; no removal decision is recorded for proposal={} target={} - a benefit verdict or run authority is not consent",
                action.as_str(),
                request.proposal,
                request.target
            );
            Ok(1)
        }
        board_hypothesis::RemovalAuthority::NotCovered { reason, .. } => {
            println!(
                "removal authority {item} action={} result=not-covered reason={reason}",
                action.as_str()
            );
            Ok(1)
        }
    }
}

/// A `--name` option parsed as a finite number.
fn number(name: &str, value: &str) -> io::Result<f64> {
    value
        .parse::<f64>()
        .map_err(|_| invalid(&format!("{name} is not a number: {value}")))
}

/// A `--name` option parsed as a whole count.
fn count(name: &str, value: &str) -> io::Result<u64> {
    value
        .parse::<u64>()
        .map_err(|_| invalid(&format!("{name} is not a whole number: {value}")))
}

fn routed_kinds(ledger: &VoteLedger) -> Vec<ObservationKind> {
    let mut kinds = Vec::new();
    for route in &ledger.routes {
        if route.target == RouteTarget::Incubator && !kinds.contains(&route.kind) {
            kinds.push(route.kind);
        }
    }
    kinds
}

fn load_decisions(path: &Path) -> io::Result<Vec<RoutedAction>> {
    let metadata = fs::metadata(path).map_err(|error| {
        invalid(&format!(
            "decisions file {} is unreadable: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(invalid(&format!(
            "decisions path {} is not a regular file",
            path.display()
        )));
    }
    if metadata.len() > MAX_DECISIONS_BYTES {
        return Err(invalid(&format!(
            "decisions file {} is {} bytes; the limit is {MAX_DECISIONS_BYTES}",
            path.display(),
            metadata.len()
        )));
    }
    let bytes = fs::read(path)?;
    let document: DecisionsDocument = serde_json::from_slice(&bytes).map_err(|error| {
        invalid(&format!(
            "decisions file {} is not a schema 1 JSON document: {error}",
            path.display()
        ))
    })?;
    if document.schema != 1 {
        return Err(invalid(&format!(
            "decisions schema {} is unsupported; this build accepts schema 1",
            document.schema
        )));
    }
    if document.decisions.is_empty() {
        return Err(invalid("decisions file has no decisions"));
    }
    if document.decisions.len() > MAX_DECISIONS {
        return Err(invalid(&format!(
            "decisions file has {} decisions; the limit is {MAX_DECISIONS}; split the batch",
            document.decisions.len()
        )));
    }
    let mut seen = BTreeSet::new();
    let mut actions = Vec::new();
    for (index, decision) in document.decisions.iter().enumerate() {
        let feedback = decision.feedback.trim();
        if feedback.is_empty() {
            return Err(invalid(&format!("decisions[{index}].feedback is empty")));
        }
        if !seen.insert(feedback.to_owned()) {
            return Err(invalid(&format!(
                "decisions[{index}].feedback repeats {feedback}; one decision per observation"
            )));
        }
        let kind = ObservationKind::parse(decision.kind.trim())
            .map_err(|error| invalid(&format!("decisions[{index}].kind: {error}")))?;
        let merge_into = decision
            .merge_into
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if merge_into == Some(feedback) {
            return Err(invalid(&format!(
                "decisions[{index}] merges {feedback} into itself"
            )));
        }
        actions.push(RoutedAction {
            feedback_id: feedback.to_owned(),
            kind,
            merge_into: merge_into.map(str::to_owned),
        });
    }
    Ok(actions)
}

fn route_text(route: board_feedback::IntakeRoute) -> &'static str {
    match route {
        board_feedback::IntakeRoute::SkillEvolution => "skill-evolution",
        board_feedback::IntakeRoute::Incubator => "incubator",
    }
}

fn vote_text(vote: &Option<VoteDecision>) -> String {
    match vote {
        None => "none(no vote: reference handoff)".to_owned(),
        Some(decision) if decision.counted => "counted".to_owned(),
        Some(decision) => decision.reason.as_str().to_owned(),
    }
}

fn kinds_text(kinds: &[ObservationKind]) -> String {
    if kinds.is_empty() {
        return "unclassified".to_owned();
    }
    kinds
        .iter()
        .map(|kind| kind.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// A bounded one-line rendering; truncation is always visible.
fn display_text(value: &str) -> String {
    const LIMIT: usize = 120;
    let total = value.chars().count();
    if total <= LIMIT {
        return value.to_owned();
    }
    let truncated: String = value.chars().take(LIMIT).collect();
    format!("{truncated}... [truncated: {total} chars total]")
}

/// Seconds since the Unix epoch: pacing freshness and expiry are evaluated at
/// the moment the ledger reads them.
fn epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// `--name value` options, each name at most once.
struct Options {
    values: Vec<(String, String)>,
}

impl Options {
    fn parse(args: &[OsString]) -> io::Result<Self> {
        let mut values: Vec<(String, String)> = Vec::new();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            let key = arg
                .to_str()
                .ok_or_else(|| invalid("feedback option names must be UTF-8"))?;
            if !key.starts_with("--") {
                return Err(invalid(&format!(
                    "unexpected feedback argument {key}; options are --name VALUE"
                )));
            }
            let value = iter
                .next()
                .ok_or_else(|| invalid(&format!("{key} requires a value")))?
                .to_str()
                .ok_or_else(|| invalid(&format!("{key} value must be UTF-8")))?;
            if values.iter().any(|(existing, _)| existing == key) {
                return Err(invalid(&format!("{key} is repeated")));
            }
            values.push((key.to_owned(), value.to_owned()));
        }
        Ok(Self { values })
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    fn required(&self, key: &str) -> io::Result<&str> {
        self.get(key)
            .ok_or_else(|| invalid(&format!("{key} is required")))
    }

    fn allowed(&self, keys: &[&str]) -> io::Result<()> {
        for (name, _) in &self.values {
            if !keys.contains(&name.as_str()) {
                return Err(invalid(&format!("unknown feedback option {name}")));
            }
        }
        Ok(())
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_owned())
}
