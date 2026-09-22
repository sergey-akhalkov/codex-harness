//! Installed `codex-harness feedback`: deterministic, model-free board
//! bookkeeping over the bd-backed algorithms in `harness_core`.
//!
//! Six verbs, each an explicit operation: `record` writes one bounded
//! observation, `list`, `ledger` and `candidates` only read, `triage` applies
//! caller-selected semantic decisions in one configured batch, and `promote`
//! performs one explicit recorded promotion. Similarity, merging and
//! consequence stay caller decisions. Thresholds and batch size come from the
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
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

const USAGE: &str = "codex-harness feedback record --project DIRECTORY --observation TEXT --scope TEXT --reporter ID --episode ID --kind lead|executor|diagnostic --parent ID [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback list --project DIRECTORY [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback ledger --project DIRECTORY --item ID [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback triage --project DIRECTORY --decisions FILE [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback candidates --project DIRECTORY [--bd FILE] [--source DIRECTORY]\ncodex-harness feedback promote --project DIRECTORY --item ID [--route backlog-task|openspec-change|kit-backlog|default] [--openspec-change NAME] [--kit-project DIRECTORY --summary TEXT --scope TEXT] [--override-consequence TEXT --override-reason TEXT] [--bd FILE] [--source DIRECTORY]\nRecords, triages, inspects and promotes board feedback through the consuming project's bd board. Thresholds and the triage batch size come from --source/global/orchestration.toml when --source is given, else from the installed kit checkout recorded by CODEX_HOME/harness/installation.json, else from the project's own global/orchestration.toml, else from the kit defaults; every verb prints the configuration source it used. The triage decisions file is strict versioned JSON: {\"schema\": 1, \"decisions\": [{\"feedback\": \"ID\", \"kind\": \"process\", \"merge_into\": \"ID or null\"}]}. Semantic grouping and consequence are caller decisions: this command adds no similarity, no model, no tracker and no implementation authority. An openspec-change promotion validates the intended change directory the OpenSpec workflow created (`openspec new change NAME`) and records its reference as the promotion target; the harness never writes into openspec/ and rerunning preserves an existing draft while it reconciles the board. A partial batch reports the applied prefix and the failing operation with a nonzero exit, and rerunning the same decisions never adds a duplicate counted vote, merge or promotion.";

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
        None => "no installed kit checkout is recorded".to_owned(),
    };
    Ok(Limits {
        checkout: project.to_path_buf(),
        account: format!("defaults({} is absent and {kit_note})", config.display()),
    })
}

/// The kit source root recorded by the normal installation. Reading is
/// bounded and executes nothing the record names; an absent, unreadable or
/// relocated record simply means no installed kit source is available.
fn installed_kit_source() -> Option<PathBuf> {
    let codex_home = std::env::var_os("CODEX_HOME")?;
    let path = PathBuf::from(codex_home).join("harness/installation.json");
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_INSTALLATION_BYTES {
        return None;
    }
    let bytes = fs::read(&path).ok()?;
    let record: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let root = PathBuf::from(record.get("sourceRoot")?.as_str()?);
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
    Ok(0)
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
