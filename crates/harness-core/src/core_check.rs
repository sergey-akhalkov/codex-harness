//! Read-only native schema-2 core check. No publication, build, download or
//! feature edit is performed; missing and legacy state is preserved.
#![cfg(windows)]

use crate::{
    agent_config, build_identity,
    config_file::ConfigSnapshot,
    core_runtime,
    installation_lock::InstallationLocks,
    installation_metadata::{InstallationMetadata, InstalledLink},
    installation_path::PathChange,
    installation_state::{PathScope, normal},
    inventory, native_launcher,
    registration::LinkType,
    registration_native as native,
};
use serde::Serialize;
use std::{collections::BTreeSet, fs, io, io::Read, path::Path, time::Duration};

#[derive(Serialize)]
pub struct CheckReport {
    pub status: &'static str,
    pub model_calls: u32,
    pub links: usize,
    pub runtime: core_runtime::RuntimeReceipt,
    /// Loop delivery report: configured roles and limits, the guidance skills
    /// a fresh session discovers the workflow through, and the board tool.
    pub orchestration: crate::orchestration_lifecycle::CheckReport,
    /// Computed shared CPU policy and coverage. Inspection does not write the
    /// policy, create a job, or sample consumption.
    pub cpu_policy: crate::core_install::CpuPolicyReport,
    /// Installed budget and coverage inspection for the manager check surface.
    /// Readback is configuration, not measured consumption. Inspection does not
    /// write policy, create a job, sample CPU, or write a protocol stream.
    pub cpu_budget: CpuBudgetStatus,
}

/// Observe an existing native schema-2 core installation. Pending recovery is
/// refused before any original CLI launch. User PATH presence is registry text
/// only; it does not prove a PowerShell function, alias or fresh-terminal lookup.
pub fn check(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
    timeout: Duration,
) -> io::Result<CheckReport> {
    for path in [codex_home, user_home, dependency_user_home] {
        normal(path)?;
    }
    if timeout.is_zero() || timeout > Duration::from_secs(300) {
        return Err(io::Error::other(
            "native core check requires a finite timeout of at most 300 seconds",
        ));
    }
    let _locks = InstallationLocks::acquire(user_home, dependency_user_home)?;
    refuse_pending(codex_home)?;
    let metadata = read_native(codex_home, user_home, dependency_user_home)?;
    let settings = metadata.settings();
    inspect_links(metadata.links())?;
    let (launch, launch_snapshot) = inspect_native_launch(&metadata)?;
    let build = launch
        .build
        .as_ref()
        .ok_or_else(|| disconnected("native-launch registration is not schema 2"))?;
    normal(build)?;
    require_native_commands(metadata.links(), build)?;
    build_identity::ordinary(&settings.codex_command)?;
    if !settings
        .codex_command
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
    {
        return Err(disconnected(
            "registered command is not a native executable",
        ));
    }
    if !same_path(&launch.upstream.executable, &settings.codex_command)? {
        return Err(disconnected(
            "native-launch upstream does not match the registered command",
        ));
    }
    let upstream_hash = build_identity::hash_file(&settings.codex_command)?;
    if launch.upstream.sha256 != upstream_hash {
        return Err(disconnected(
            "native-launch upstream hash does not match the original executable",
        ));
    }
    if settings.codex_command.canonicalize()? == build.join("codex.exe").canonicalize()? {
        return Err(disconnected(
            "native-launch registration is not schema 2 core config",
        ));
    }
    require_path(
        settings.path_scope,
        &settings.codex_home.join("harness/bin"),
    )?;
    require_current_source(build, &settings.source_root)?;
    let inventory = inventory::read(&settings.source_root, codex_home, user_home)?;
    require_data_links(metadata.links(), &inventory.links)?;
    agent_config::check(codex_home, &inventory.agents)?;
    let orchestration =
        crate::orchestration_lifecycle::check(&settings.source_root, codex_home, user_home, false)?;
    let instructions =
        read_instructions(&settings.source_root.join(&inventory.manifest.instructions))?;
    let runtime = core_runtime::verify(
        &settings.codex_command,
        &codex_home.join("harness/bin/codex.exe"),
        codex_home,
        &instructions,
        timeout,
    )?;
    metadata.verify_unchanged()?;
    inspect_links(metadata.links())?;
    launch_snapshot.verify_unchanged()?;
    require_path(
        settings.path_scope,
        &settings.codex_home.join("harness/bin"),
    )?;
    require_current_source(build, &settings.source_root)?;
    let current = inventory::read(&settings.source_root, codex_home, user_home)?;
    require_data_links(metadata.links(), &current.links)?;
    if read_instructions(&settings.source_root.join(&current.manifest.instructions))?
        != instructions
    {
        return Err(disconnected(
            "instruction source changed during runtime observation",
        ));
    }
    let (launch_after, _) = inspect_native_launch(&metadata)?;
    if launch_after.schema != launch.schema
        || launch_after.build != launch.build
        || launch_after.upstream.executable != launch.upstream.executable
        || launch_after.upstream.sha256 != launch.upstream.sha256
    {
        return Err(disconnected(
            "native-launch registration changed during runtime observation",
        ));
    }
    let mut routes = Vec::new();
    for link in metadata.links() {
        for path in [&link.object.path, &link.object.target] {
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
            {
                routes.push(path.clone());
            }
        }
    }
    if let Some(build) = &launch.build {
        for name in build_identity::BINARIES {
            routes.push(build.join(name));
        }
    }
    let cpu_policy = crate::core_install::inspect_cpu_policy(&routes);
    let cpu_budget = cpu_budget::inspect_installed_budget(&routes);
    Ok(CheckReport {
        status: "connected",
        model_calls: 0,
        links: metadata.links().len(),
        runtime,
        orchestration,
        cpu_policy,
        cpu_budget,
    })
}

mod cpu_budget {
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs, io,
        io::Read,
        path::{Component, Path, PathBuf},
    };

    use serde::Serialize;

    const POLICY_FILE: &str = "shared-cpu-policy.json";
    const OWNERSHIP_RECORD: &str = "cpu-budget.json";
    const POLICY_SCHEMA: u64 = 1;
    const MAX_RECORD: u64 = 64 * 1024;
    const MAX_PROCESSES: usize = 8192;
    const MAX_ROUTES: usize = 1024;
    const MAX_WIDE: usize = 32_768;
    const SHARED_CPU_JOB_PREFIX: &str = "CodingAgentsHarness.SharedCpu.";
    const JOB_OBJECT_QUERY: u32 = 0x0004;

    /// Read-only installed CPU budget and coverage report.
    ///
    /// `measured_consumption` is always `not-sampled`. Kernel rate, hard-cap
    /// flags and membership counts are configuration readback, not a
    /// measurement of enforcement.
    #[derive(Debug, Serialize)]
    pub struct CpuBudgetStatus {
        pub action: &'static str,
        pub wrote_policy: bool,
        pub created_job: bool,
        pub model_calls: u32,
        pub measured_consumption: &'static str,
        pub enforcement: &'static str,
        pub activation: &'static str,
        pub configured: ConfiguredBudget,
        pub effective: EffectiveKernel,
        pub routes: Vec<RouteCoverage>,
        pub process_group: ProcessGroupReport,
        pub exceptions: Vec<Sighting>,
        pub degraded_starts: Vec<Sighting>,
        pub unknown_members: Vec<Sighting>,
        pub rejected_exception_markers: Vec<Sighting>,
        pub restart_boundary: String,
        pub coverage_action: String,
        pub command_line_observation: &'static str,
        pub summary: String,
    }

    #[derive(Debug, Serialize)]
    pub struct ConfiguredBudget {
        pub source: &'static str,
        pub policy_path: String,
        pub present: bool,
        pub usable: bool,
        pub ceiling_percent: Option<f64>,
        pub detail: String,
    }

    #[derive(Debug, Serialize)]
    pub struct EffectiveKernel {
        pub source: &'static str,
        pub opened: bool,
        pub job: String,
        pub cpu_rate: Option<u32>,
        pub rate_control_enabled: Option<bool>,
        pub cpu_hard_cap: Option<bool>,
        pub active_processes: Option<u32>,
        pub listed_members: Option<usize>,
        pub membership_list_complete: bool,
        pub kill_on_close: Option<bool>,
        pub breakaway_ok: Option<bool>,
        pub silent_breakaway_ok: Option<bool>,
        pub detail: String,
        /// Process ids read from the job. Not part of the serialized check
        /// report: membership is reported through routes and sightings.
        #[serde(skip)]
        pub member_pids: Vec<u32>,
    }

    #[derive(Debug, Serialize)]
    pub struct RouteCoverage {
        pub path: String,
        pub state: &'static str,
        pub covered_pids: Vec<u32>,
        pub outside_pids: Vec<u32>,
        pub inaccessible_pids: Vec<u32>,
    }

    #[derive(Debug, Serialize)]
    pub struct ProcessGroupReport {
        pub name: String,
        pub opened: bool,
        pub member_count: Option<u32>,
        pub detail: String,
    }

    #[derive(Debug, Serialize)]
    pub struct Sighting {
        pub pid: u32,
        pub image: String,
        pub kind: &'static str,
        pub detail: String,
    }

    /// Inspect one account directory and the supplied installed entry points.
    ///
    /// This does not write the policy, create a directory or job, assign a
    /// process, sample CPU consumption, start a workload, or write stdout or
    /// stderr. Callers that surface the report, including `check`, own the
    /// established output channel.
    pub fn inspect_cpu_budget(account: &Path, routes: &[PathBuf]) -> CpuBudgetStatus {
        if let Err(detail) = validate_account(account) {
            return unavailable(&detail);
        }
        match fs::symlink_metadata(account) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return unavailable(
                    "shared CPU policy account is absent; inspection did not create it",
                );
            }
            Err(error) => {
                return unavailable(&format!(
                    "shared CPU policy account was not opened: {error}"
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return unavailable("shared CPU policy account is not a directory; preserving it");
            }
            Ok(_) => {}
        }
        if let Err(error) = crate::build_identity::ordinary(account) {
            return unavailable(&format!("shared CPU policy account was preserved: {error}"));
        }
        match account.canonicalize() {
            Ok(canonical) => inspect_canonical(&canonical, routes),
            Err(error) => unavailable(&format!(
                "shared CPU policy account was not canonicalized: {error}"
            )),
        }
    }

    pub(super) fn inspect_installed_budget(routes: &[PathBuf]) -> CpuBudgetStatus {
        match selected_account() {
            Ok(account) => inspect_cpu_budget(&account, routes),
            Err(detail) => unavailable(&detail),
        }
    }

    fn selected_account() -> Result<PathBuf, String> {
        #[cfg(test)]
        {
            let Some(value) = std::env::var_os(crate::process::CPU_BUDGET_ACCOUNT_ENV)
                .filter(|value| !value.is_empty())
            else {
                return Err(
                    "shared CPU budget account was not selected; inspection did not open the default account or create a job"
                        .to_owned(),
                );
            };
            crate::process::cpu_budget_directory(Some(Path::new(&value)))
                .map_err(|error| error.to_string())
        }
        #[cfg(not(test))]
        {
            crate::process::cpu_budget_directory(None).map_err(|error| error.to_string())
        }
    }

    fn validate_account(account: &Path) -> Result<(), String> {
        if !account.is_absolute()
            || account
                .components()
                .any(|part| part == Component::ParentDir)
        {
            return Err(
                "the shared CPU budget account directory must be absolute and normalized; inspection did not create it"
                    .to_owned(),
            );
        }
        Ok(())
    }

    fn inspect_canonical(account: &Path, routes: &[PathBuf]) -> CpuBudgetStatus {
        let configured = read_configured(account);
        let effective = open_kernel(account);
        let (labels, keys, routes_truncated) = route_index(routes);
        let probe = command_probe();
        let observation = if probe.is_some() {
            "available"
        } else {
            "unavailable"
        };
        let members = effective.opened.then(|| {
            (
                effective
                    .member_pids
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>(),
                effective.membership_list_complete,
            )
        });
        let enumeration = process_ids();
        let (seen, enumeration_error) = match enumeration {
            Ok(pids) => (collect_seen(&pids, &keys, members.as_ref(), probe), None),
            Err(error) => (Vec::new(), Some(error.to_string())),
        };
        classify(
            configured,
            effective,
            &labels,
            seen,
            observation,
            routes_truncated,
            enumeration_error,
        )
    }

    struct Seen {
        pid: u32,
        image: Option<PathBuf>,
        route_index: Option<usize>,
        member: Option<bool>,
        marker: Option<bool>,
        parent: u32,
    }

    fn collect_seen(
        pids: &[u32],
        keys: &BTreeMap<String, usize>,
        members: Option<&(BTreeSet<u32>, bool)>,
        probe: Option<&CommandProbe>,
    ) -> Vec<Seen> {
        let mut seen = Vec::new();
        let mut known = BTreeSet::new();
        for pid in pids {
            if !known.insert(*pid) {
                continue;
            }
            if let Some(item) = consider(*pid, keys, members) {
                seen.push(item);
            }
        }
        if let Some((set, _)) = members {
            for pid in set {
                if known.insert(*pid)
                    && let Some(item) = consider(*pid, keys, members)
                {
                    seen.push(item);
                }
            }
        }
        fill_markers(&mut seen, probe);
        seen
    }

    fn consider(
        pid: u32,
        keys: &BTreeMap<String, usize>,
        members: Option<&(BTreeSet<u32>, bool)>,
    ) -> Option<Seen> {
        let image = image_path(pid).ok();
        let route_index = image.as_ref().and_then(|path| route_for_image(path, keys));
        let member = members.map(|(set, complete)| {
            if set.contains(&pid) {
                Some(true)
            } else if *complete {
                Some(false)
            } else {
                None
            }
        });
        let member = member.unwrap_or(None);
        if route_index.is_none() && member != Some(true) {
            return None;
        }
        Some(Seen {
            pid,
            image,
            route_index,
            member,
            marker: None,
            parent: 0,
        })
    }

    fn fill_markers(seen: &mut [Seen], probe: Option<&CommandProbe>) {
        let Some(probe) = probe else {
            return;
        };
        let mut reads = 0usize;
        for item in seen.iter_mut() {
            if item.route_index.is_none() {
                continue;
            }
            if reads == MAX_ROUTES {
                break;
            }
            reads += 1;
            if let Some((marker, parent)) = read_process_marker(probe, item.pid) {
                item.marker = Some(marker);
                item.parent = parent;
            }
        }
        let parents: Vec<(usize, u32)> = seen
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.member == Some(false)
                    && item.route_index.is_some()
                    && item.marker == Some(false)
                    && item.parent != 0
            })
            .map(|(index, item)| (index, item.parent))
            .collect();
        for (index, parent) in parents {
            if seen
                .iter()
                .any(|item| item.pid == parent && item.marker == Some(true))
                || read_process_marker(probe, parent).is_some_and(|(marker, _)| marker)
            {
                seen[index].marker = Some(true);
            }
        }
    }

    fn classify(
        configured: ConfiguredBudget,
        mut effective: EffectiveKernel,
        labels: &[String],
        seen: Vec<Seen>,
        observation: &'static str,
        routes_truncated: bool,
        enumeration_error: Option<String>,
    ) -> CpuBudgetStatus {
        let mut routes = labels
            .iter()
            .map(|path| RouteAccum {
                path: path.clone(),
                covered: Vec::new(),
                outside: Vec::new(),
                inaccessible: Vec::new(),
                exceptions: Vec::new(),
            })
            .collect::<Vec<_>>();
        let mut exceptions = Vec::new();
        let mut degraded = Vec::new();
        let mut unknown = Vec::new();
        let mut rejected = Vec::new();
        for item in &seen {
            let image = item
                .image
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "unreadable".to_owned());
            match item.member {
                Some(true) if item.marker == Some(true) => {
                    rejected.push(sighting(
                        item.pid,
                        &image,
                        "rejected-marker",
                        "an uncapped marker is present, but the process remains in the shared group; it is not an uncapped exception",
                    ));
                    if let Some(index) = item.route_index {
                        routes[index].covered.push(item.pid);
                    } else {
                        unknown.push(sighting(
                            item.pid,
                            &image,
                            "unknown-member",
                            "unknown member of the account job; image is not a known entry point; not counted as covered",
                        ));
                    }
                }
                Some(true) => {
                    if let Some(index) = item.route_index {
                        routes[index].covered.push(item.pid);
                    } else {
                        unknown.push(sighting(
                            item.pid,
                            &image,
                            "unknown-member",
                            "unknown member of the account job; image is not a known entry point; not counted as covered",
                        ));
                    }
                }
                Some(false) if item.marker == Some(true) => {
                    exceptions.push(sighting(
                        item.pid,
                        &image,
                        "explicit-exception",
                        "explicit uncapped invocation; combined host agent load can exceed the shared ceiling while it runs; other sessions keep their allowance; not counted as covered",
                    ));
                    if let Some(index) = item.route_index {
                        routes[index].exceptions.push(item.pid);
                    }
                }
                Some(false) if item.route_index.is_some() && item.marker == Some(false) => {
                    let index = item.route_index.unwrap_or(0);
                    degraded.push(sighting(
                        item.pid,
                        &image,
                        "degraded-start",
                        "warned degraded coverage: this process is outside verified admission and is not an explicit uncapped invocation; the shared cap is not guaranteed; other sessions keep their allowance; restart it after its work can stop",
                    ));
                    routes[index].outside.push(item.pid);
                }
                _ if item.route_index.is_some() => {
                    let index = item.route_index.unwrap_or(0);
                    unknown.push(sighting(
                        item.pid,
                        &image,
                        "inaccessible",
                        "membership or command line was not readable; not counted as covered",
                    ));
                    routes[index].inaccessible.push(item.pid);
                }
                _ => {}
            }
        }
        let route_reports = if enumeration_error.is_some() {
            labels
                .iter()
                .map(|path| RouteCoverage {
                    path: path.clone(),
                    state: "unknown",
                    covered_pids: Vec::new(),
                    outside_pids: Vec::new(),
                    inaccessible_pids: Vec::new(),
                })
                .collect()
        } else {
            routes.iter().map(RouteAccum::report).collect()
        };
        effective.member_pids.clear();
        finish(
            configured,
            effective,
            route_reports,
            exceptions,
            degraded,
            unknown,
            rejected,
            observation,
            routes_truncated,
            enumeration_error,
        )
    }

    struct RouteAccum {
        path: String,
        covered: Vec<u32>,
        outside: Vec<u32>,
        inaccessible: Vec<u32>,
        exceptions: Vec<u32>,
    }

    impl RouteAccum {
        fn state(&self) -> &'static str {
            if !self.inaccessible.is_empty() {
                "unknown"
            } else if !self.outside.is_empty() {
                "uncovered"
            } else if !self.exceptions.is_empty() && self.covered.is_empty() {
                "exception"
            } else if !self.exceptions.is_empty() {
                "partial"
            } else if !self.covered.is_empty() {
                "covered"
            } else {
                "not_running"
            }
        }

        fn report(&self) -> RouteCoverage {
            RouteCoverage {
                path: self.path.clone(),
                state: self.state(),
                covered_pids: self.covered.clone(),
                outside_pids: self.outside.clone(),
                inaccessible_pids: self.inaccessible.clone(),
            }
        }
    }

    // The report is one value; splitting the assembler would only rename the same inputs.
    #[allow(clippy::too_many_arguments)]
    fn finish(
        configured: ConfiguredBudget,
        effective: EffectiveKernel,
        routes: Vec<RouteCoverage>,
        exceptions: Vec<Sighting>,
        degraded: Vec<Sighting>,
        unknown: Vec<Sighting>,
        rejected: Vec<Sighting>,
        observation: &'static str,
        routes_truncated: bool,
        enumeration_error: Option<String>,
    ) -> CpuBudgetStatus {
        let enumeration_ok = enumeration_error.is_none() && !routes_truncated;
        let enforcement = enforcement_of(
            &configured,
            &effective,
            &routes,
            &exceptions,
            &degraded,
            &unknown,
            enumeration_ok,
        );
        let routes_settled = routes
            .iter()
            .all(|route| matches!(route.state, "covered" | "not_running"));
        let activation = if enforcement == "capped"
            && exceptions.is_empty()
            && degraded.is_empty()
            && routes_settled
            && enumeration_ok
        {
            "complete"
        } else {
            "incomplete"
        };
        let restart_boundary = restart_boundary(&degraded, enumeration_error.as_deref());
        let coverage_action = coverage_action(
            &effective,
            &configured,
            &exceptions,
            &degraded,
            &unknown,
            enumeration_error.as_deref(),
            routes_truncated,
        );
        let process_group = ProcessGroupReport {
            name: effective.job.clone(),
            opened: effective.opened,
            member_count: effective.active_processes,
            detail: effective.detail.clone(),
        };
        let summary = format!(
            "{}; {}; enforcement {enforcement}; activation {activation}; covered routes {}; uncovered routes {}; exceptions {}; degraded starts {}; unknown members {}; {}",
            configured.detail,
            effective.detail,
            routes
                .iter()
                .filter(|route| route.state == "covered")
                .count(),
            routes
                .iter()
                .filter(|route| route.state == "uncovered")
                .count(),
            exceptions.len(),
            degraded.len(),
            unknown.len(),
            coverage_action,
        );
        CpuBudgetStatus {
            action: "inspected",
            wrote_policy: false,
            created_job: false,
            model_calls: 0,
            measured_consumption: "not-sampled",
            enforcement,
            activation,
            configured,
            effective,
            routes,
            process_group,
            exceptions,
            degraded_starts: degraded,
            unknown_members: unknown,
            rejected_exception_markers: rejected,
            restart_boundary,
            coverage_action,
            command_line_observation: observation,
            summary,
        }
    }

    fn enforcement_of(
        configured: &ConfiguredBudget,
        effective: &EffectiveKernel,
        routes: &[RouteCoverage],
        exceptions: &[Sighting],
        degraded: &[Sighting],
        unknown: &[Sighting],
        enumeration_ok: bool,
    ) -> &'static str {
        let inaccessible = !enumeration_ok
            || routes.iter().any(|route| route.state == "unknown")
            || unknown.iter().any(|item| item.kind == "inaccessible")
            || !effective.opened
            || !effective.membership_list_complete;
        if inaccessible {
            return "unknown";
        }
        let mismatch = !rate_matches(configured, effective);
        let has_exception = !exceptions.is_empty();
        let has_degraded = !degraded.is_empty() || mismatch;
        if has_exception && (has_degraded || routes.iter().any(|route| route.state == "covered")) {
            "mixed"
        } else if has_exception {
            "explicit-uncapped"
        } else if has_degraded {
            "degraded"
        } else if effective.cpu_hard_cap == Some(true) && rate_matches(configured, effective) {
            "capped"
        } else {
            "unknown"
        }
    }

    fn rate_matches(configured: &ConfiguredBudget, effective: &EffectiveKernel) -> bool {
        let Some(percent) = configured.ceiling_percent.filter(|_| configured.usable) else {
            return false;
        };
        let Some(expected) = expected_rate(percent) else {
            return false;
        };
        effective.cpu_rate == Some(expected)
            && effective.rate_control_enabled == Some(true)
            && effective.cpu_hard_cap == Some(true)
    }

    fn expected_rate(percent: f64) -> Option<u32> {
        if percent.is_finite() && (0.01..=100.0).contains(&percent) {
            Some((percent * 100.0).floor() as u32)
        } else {
            None
        }
    }

    fn restart_boundary(degraded: &[Sighting], enumeration_error: Option<&str>) -> String {
        if let Some(error) = enumeration_error {
            return format!(
                "process enumeration did not complete ({error}); activation stays incomplete and no process was terminated"
            );
        }
        if degraded.is_empty() {
            return "no retained ordinary route is running outside the account job; no restart is required and no process was terminated"
                .to_owned();
        }
        degraded
            .iter()
            .map(|item| {
                format!(
                    "pid {} image {} is outside verified admission; restart that process after its work can stop; inspection does not terminate it",
                    item.pid, item.image
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn coverage_action(
        effective: &EffectiveKernel,
        configured: &ConfiguredBudget,
        exceptions: &[Sighting],
        degraded: &[Sighting],
        unknown: &[Sighting],
        enumeration_error: Option<&str>,
        routes_truncated: bool,
    ) -> String {
        let mut parts = Vec::new();
        if let Some(error) = enumeration_error {
            parts.push(format!(
                "process enumeration did not complete ({error}); do not treat coverage as complete; no process was terminated"
            ));
        }
        if routes_truncated {
            parts.push(
                "the route list exceeded its bound; truncated routes were not counted as covered"
                    .to_owned(),
            );
        }
        if !effective.opened {
            parts.push("the account job was not opened; inspection did not create one".to_owned());
        } else if !rate_matches(configured, effective) {
            parts.push(
                "the kernel rate does not match the policy record; this is configuration disagreement, not a consumption measurement"
                    .to_owned(),
            );
        }
        if !degraded.is_empty() {
            parts.push(
                "restart degraded or uncovered processes after their work can stop; inspection does not terminate them"
                    .to_owned(),
            );
        }
        if !exceptions.is_empty() {
            parts.push(
                "an explicit uncapped invocation is active; it is not a failed admission and does not change other sessions"
                    .to_owned(),
            );
        }
        if !unknown.is_empty() {
            parts.push("unknown or inaccessible members were not counted as covered".to_owned());
        }
        if parts.is_empty() {
            "no action is required for coverage; no process was terminated".to_owned()
        } else {
            parts.join("; ")
        }
    }

    fn sighting(pid: u32, image: &str, kind: &'static str, detail: &str) -> Sighting {
        Sighting {
            pid,
            image: image.to_owned(),
            kind,
            detail: detail.to_owned(),
        }
    }

    fn unavailable(detail: &str) -> CpuBudgetStatus {
        let configured = ConfiguredBudget {
            source: "policy-record",
            policy_path: String::new(),
            present: false,
            usable: false,
            ceiling_percent: None,
            detail: detail.to_owned(),
        };
        let effective = not_opened("", detail);
        finish(
            configured,
            effective,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            "unavailable",
            false,
            None,
        )
    }

    fn read_configured(account: &Path) -> ConfiguredBudget {
        let path = account.join(POLICY_FILE);
        match read_record(&path) {
            Ok(None) => ConfiguredBudget {
                source: "policy-record",
                policy_path: path.display().to_string(),
                present: false,
                usable: false,
                ceiling_percent: None,
                detail: "policy record is absent; inspection did not create it".to_owned(),
            },
            Ok(Some(bytes)) => {
                let (ceiling, usable, detail) = parse_policy(&bytes);
                ConfiguredBudget {
                    source: "policy-record",
                    policy_path: path.display().to_string(),
                    present: true,
                    usable,
                    ceiling_percent: ceiling,
                    detail,
                }
            }
            Err(detail) => ConfiguredBudget {
                source: "policy-record",
                policy_path: path.display().to_string(),
                present: true,
                usable: false,
                ceiling_percent: None,
                detail,
            },
        }
    }

    fn parse_policy(bytes: &[u8]) -> (Option<f64>, bool, String) {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
            return (
                None,
                false,
                "policy record is unreadable; preserving it".to_owned(),
            );
        };
        let schema_ok = value.get("schema").and_then(|item| item.as_u64()) == Some(POLICY_SCHEMA);
        let ceiling = value
            .get("ceiling_percent")
            .and_then(|item| item.as_f64())
            .filter(|percent| percent.is_finite() && (0.01..=100.0).contains(percent));
        let usable = schema_ok && ceiling.is_some();
        let detail = match (usable, ceiling) {
            (true, Some(percent)) => {
                format!("configured ceiling {percent}% from the policy record")
            }
            (false, Some(_)) => {
                "policy record ceiling was read but the record is not usable; preserving it"
                    .to_owned()
            }
            _ => "policy record has no usable ceiling; preserving it".to_owned(),
        };
        (ceiling, usable, detail)
    }

    fn open_kernel(account: &Path) -> EffectiveKernel {
        let expected = shared_cpu_job_name(account);
        let job_name = match read_job_name(account, &expected) {
            Ok(None) => return not_opened(&expected, "no ownership record"),
            Ok(Some(name)) => name,
            Err(detail) => return not_opened(&expected, &detail),
        };
        match query_job(&job_name) {
            Ok(read) => read.into_effective(),
            Err(error) => not_opened(
                &job_name,
                &format!("kernel configuration was not opened for job {job_name}: {error}"),
            ),
        }
    }

    fn not_opened(job: &str, reason: &str) -> EffectiveKernel {
        EffectiveKernel {
            source: "not-opened",
            opened: false,
            job: job.to_owned(),
            cpu_rate: None,
            rate_control_enabled: None,
            cpu_hard_cap: None,
            active_processes: None,
            listed_members: None,
            membership_list_complete: false,
            kill_on_close: None,
            breakaway_ok: None,
            silent_breakaway_ok: None,
            member_pids: Vec::new(),
            detail: format!(
                "{reason}; inspection did not create a job; this is configuration, not measured consumption"
            ),
        }
    }

    struct KernelRead {
        job: String,
        rate: u32,
        enabled: bool,
        hard_cap: bool,
        active: u32,
        kill_on_close: bool,
        breakaway_ok: bool,
        silent_breakaway_ok: bool,
        members: Vec<u32>,
        list_complete: bool,
        list_error: Option<String>,
    }

    impl KernelRead {
        fn into_effective(self) -> EffectiveKernel {
            let list_note = self
                .list_error
                .as_deref()
                .map(|error| format!("; membership list was not read: {error}"))
                .unwrap_or_default();
            EffectiveKernel {
                source: "kernel-readback",
                opened: true,
                job: self.job.clone(),
                cpu_rate: Some(self.rate),
                rate_control_enabled: Some(self.enabled),
                cpu_hard_cap: Some(self.hard_cap),
                active_processes: Some(self.active),
                listed_members: self.list_error.is_none().then_some(self.members.len()),
                membership_list_complete: self.list_complete,
                kill_on_close: Some(self.kill_on_close),
                breakaway_ok: Some(self.breakaway_ok),
                silent_breakaway_ok: Some(self.silent_breakaway_ok),
                member_pids: self.members,
                detail: format!(
                    "kernel configuration readback: job {} cpu_rate {} hard_cap {} active_processes {}{list_note}; this is configuration, not measured consumption",
                    self.job, self.rate, self.hard_cap, self.active
                ),
            }
        }
    }

    fn read_job_name(account: &Path, expected: &str) -> Result<Option<String>, String> {
        let path = account.join(OWNERSHIP_RECORD);
        let Some(bytes) = read_record(&path)? else {
            return Ok(None);
        };
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("ownership record is unreadable ({error}); preserving it"))?;
        let Some(job) = value.get("job").and_then(|item| item.as_str()) else {
            return Err("ownership record has no job name; preserving it".to_owned());
        };
        let recorded_account = value
            .get("account")
            .and_then(|item| item.as_str())
            .unwrap_or("");
        if normalize(Path::new(recorded_account)) != normalize(account) {
            return Err(
                "ownership record names a different account; inspection did not open or create a job"
                    .to_owned(),
            );
        }
        if job != expected || !job_name_is_bounded(job) {
            return Err(format!(
                "ownership record job {job} does not match this account's job {expected}; inspection did not open or create a job"
            ));
        }
        Ok(Some(job.to_owned()))
    }

    fn job_name_is_bounded(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 128
            && name.is_ascii()
            && !name.contains(['\\', '/', '\0'])
    }

    fn read_record(path: &Path) -> Result<Option<Vec<u8>>, String> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("{} was not opened: {error}", path.display())),
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!("{} is a link; preserving it", path.display()));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(format!("{} is not a file; preserving it", path.display()));
            }
            Ok(metadata) if metadata.len() > MAX_RECORD => {
                return Err(format!(
                    "{} exceeds its bound; preserving it",
                    path.display()
                ));
            }
            Ok(_) => {}
        }
        if let Err(error) = crate::build_identity::ordinary(path) {
            return Err(format!("{} was preserved: {error}", path.display()));
        }
        let file = fs::File::open(path)
            .map_err(|error| format!("{} was not opened: {error}", path.display()))?;
        let mut bytes = Vec::new();
        file.take(MAX_RECORD + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("{} was not read: {error}", path.display()))?;
        if bytes.len() as u64 > MAX_RECORD {
            return Err(format!(
                "{} exceeds its bound; preserving it",
                path.display()
            ));
        }
        Ok(Some(bytes))
    }

    fn route_index(routes: &[PathBuf]) -> (Vec<String>, BTreeMap<String, usize>, bool) {
        let truncated = routes.len() > MAX_ROUTES;
        let mut labels = Vec::new();
        let mut keys = BTreeMap::new();
        for (index, route) in routes.iter().take(MAX_ROUTES).enumerate() {
            labels.push(route.display().to_string());
            for key in route_keys(route) {
                keys.entry(key).or_insert(index);
            }
        }
        (labels, keys, truncated)
    }

    fn route_keys(route: &Path) -> Vec<String> {
        let mut keys = vec![normalize(route)];
        if let Ok(target) = fs::read_link(route) {
            keys.push(normalize(&target));
        }
        if let Ok(canonical) = route.canonicalize() {
            keys.push(normalize(&canonical));
        }
        keys
    }

    fn route_for_image(image: &Path, keys: &BTreeMap<String, usize>) -> Option<usize> {
        let mut candidates = vec![normalize(image)];
        if let Ok(canonical) = image.canonicalize() {
            candidates.push(normalize(&canonical));
        }
        candidates
            .into_iter()
            .find_map(|key| keys.get(&key).copied())
    }

    fn normalize(path: &Path) -> String {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_start_matches(r"\\?\")
            .to_ascii_lowercase()
    }

    fn shared_cpu_job_name(directory: &Path) -> String {
        let account = directory.to_string_lossy().to_lowercase();
        format!("{SHARED_CPU_JOB_PREFIX}{:016x}", budget_key(&account))
    }

    fn budget_key(text: &str) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for byte in text.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    fn query_job(name: &str) -> io::Result<KernelRead> {
        use std::mem::{size_of, zeroed};
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
            JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectBasicAccountingInformation, JobObjectCpuRateControlInformation,
            JobObjectExtendedLimitInformation, OpenJobObjectW, QueryInformationJobObject,
        };
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: `wide` is NUL-terminated. A successful open returns an owned query handle.
        // JOB_OBJECT_QUERY cannot assign, configure or terminate the object.
        let handle = owned(unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, wide.as_ptr()) })?;
        let raw = handle.as_raw_handle();
        let mut cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = unsafe { zeroed() };
        let mut extended: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
        // SAFETY: each query writes one documented structure into a live local buffer.
        let cpu_ok = unsafe {
            QueryInformationJobObject(
                raw,
                JobObjectCpuRateControlInformation,
                (&mut cpu as *mut JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
                size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        let extended_ok = unsafe {
            QueryInformationJobObject(
                raw,
                JobObjectExtendedLimitInformation,
                (&mut extended as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        let accounting_ok = unsafe {
            QueryInformationJobObject(
                raw,
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if cpu_ok == 0 || extended_ok == 0 || accounting_ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let enabled = cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_ENABLE != 0;
        let hard_cap = cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP != 0;
        // SAFETY: the enable flag selects the CpuRate union member; otherwise the rate is unused.
        let rate = if enabled {
            unsafe { cpu.Anonymous.CpuRate }
        } else {
            0
        };
        let flags = extended.BasicLimitInformation.LimitFlags;
        let (members, list_complete, list_error) = match job_members(raw) {
            Ok((members, complete)) => (members, complete, None),
            Err(error) => (Vec::new(), false, Some(error.to_string())),
        };
        Ok(KernelRead {
            job: name.to_owned(),
            rate,
            enabled,
            hard_cap,
            active: accounting.ActiveProcesses,
            kill_on_close: flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE != 0,
            breakaway_ok: flags & JOB_OBJECT_LIMIT_BREAKAWAY_OK != 0,
            silent_breakaway_ok: flags & JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK != 0,
            members,
            list_complete,
            list_error,
        })
    }

    fn job_members(handle: windows_sys::Win32::Foundation::HANDLE) -> io::Result<(Vec<u32>, bool)> {
        use std::mem::size_of;
        use windows_sys::Win32::Foundation::ERROR_MORE_DATA;
        use windows_sys::Win32::System::JobObjects::{
            JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList, QueryInformationJobObject,
        };
        let mut capacity = 32usize;
        loop {
            let bytes = size_of::<u32>()
                .saturating_mul(2)
                .saturating_add(capacity.saturating_mul(size_of::<usize>()));
            let Ok(bytes_u32) = u32::try_from(bytes) else {
                return Err(io::Error::other(
                    "job membership list exceeded its bound; no process was terminated",
                ));
            };
            let mut buffer = vec![0u64; bytes.div_ceil(size_of::<u64>())];
            let mut returned = 0u32;
            // SAFETY: `buffer` is pointer-aligned and writable for `bytes_u32` bytes.
            // The call only queries the job; it does not assign or terminate a process.
            let ok = unsafe {
                QueryInformationJobObject(
                    handle,
                    JobObjectBasicProcessIdList,
                    buffer.as_mut_ptr().cast(),
                    bytes_u32,
                    &mut returned,
                )
            };
            if ok == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(i32::try_from(ERROR_MORE_DATA).unwrap_or(234))
                    && capacity < MAX_PROCESSES
                {
                    capacity = capacity.saturating_mul(2).min(MAX_PROCESSES);
                    continue;
                }
                return Err(error);
            }
            // SAFETY: a successful query wrote a process-id list header at the buffer start.
            let header = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
            let listed = header.NumberOfProcessIdsInList as usize;
            let assigned = header.NumberOfAssignedProcesses as usize;
            if listed > capacity {
                return Err(io::Error::other(
                    "job membership list was truncated unexpectedly; no process was terminated",
                ));
            }
            // SAFETY: `listed` ids were written in the flexible array and fit in `buffer`.
            let ids = unsafe { std::slice::from_raw_parts(header.ProcessIdList.as_ptr(), listed) };
            let mut pids = Vec::new();
            for id in ids {
                if let Ok(pid) = u32::try_from(*id)
                    && pid != 0
                {
                    pids.push(pid);
                }
            }
            return Ok((pids, assigned == listed));
        }
    }

    fn process_ids() -> io::Result<Vec<u32>> {
        use std::mem::size_of;
        use windows_sys::Win32::System::ProcessStatus::K32EnumProcesses;
        let mut capacity = 1024usize;
        loop {
            let mut ids = vec![0u32; capacity];
            let Ok(bytes) = u32::try_from(ids.len().saturating_mul(size_of::<u32>())) else {
                return Err(io::Error::other(
                    "process enumeration exceeded its bound; no process was terminated",
                ));
            };
            let mut used = 0u32;
            // SAFETY: `ids` is a writable buffer of `bytes` bytes for this call.
            if unsafe { K32EnumProcesses(ids.as_mut_ptr(), bytes, &mut used) } == 0
                || used > bytes
                || !used.is_multiple_of(size_of::<u32>() as u32)
            {
                return Err(io::Error::other(
                    "process enumeration failed; no process was terminated",
                ));
            }
            if used == bytes {
                if capacity == MAX_PROCESSES {
                    return Err(io::Error::other(
                        "process enumeration exceeded its bound; no process was terminated",
                    ));
                }
                capacity = capacity.saturating_mul(2).min(MAX_PROCESSES);
                continue;
            }
            ids.truncate(used as usize / size_of::<u32>());
            ids.retain(|pid| *pid != 0);
            return Ok(ids);
        }
    }

    fn image_path(pid: u32) -> io::Result<PathBuf> {
        use std::os::windows::ffi::OsStringExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        };
        // SAFETY: a successful open returns an owned query handle; failure is null.
        let handle = owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })?;
        let mut buffer = vec![0u16; 1024];
        let mut length = buffer.len() as u32;
        // SAFETY: `buffer` is writable for `length` wide characters and the handle is live.
        let ok = unsafe {
            QueryFullProcessImageNameW(handle.as_raw_handle(), 0, buffer.as_mut_ptr(), &mut length)
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        buffer.truncate(length as usize);
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer)))
    }

    fn owned(
        handle: windows_sys::Win32::Foundation::HANDLE,
    ) -> io::Result<std::os::windows::io::OwnedHandle> {
        use std::os::windows::io::{FromRawHandle, OwnedHandle};
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `handle` came from a successful native open and is not a pseudo-handle.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    struct CommandProbe {
        query: NtQuery,
    }

    type NtQuery = unsafe extern "system" fn(
        windows_sys::Win32::Foundation::HANDLE,
        i32,
        *mut std::ffi::c_void,
        u32,
        *mut u32,
    ) -> i32;

    fn command_probe() -> Option<&'static CommandProbe> {
        static SLOT: std::sync::OnceLock<Option<CommandProbe>> = std::sync::OnceLock::new();
        SLOT.get_or_init(CommandProbe::validated).as_ref()
    }

    impl CommandProbe {
        fn validated() -> Option<Self> {
            #[cfg(not(target_arch = "x86_64"))]
            {
                return None;
            }
            #[cfg(target_arch = "x86_64")]
            {
                let query = load_query()?;
                let probe = Self { query };
                let handle = unsafe { windows_sys::Win32::System::Threading::GetCurrentProcess() };
                let units = probe.read_units(handle)?;
                let local = unsafe { windows_sys::Win32::System::Environment::GetCommandLineW() };
                if local.is_null() || units.0.is_empty() {
                    return None;
                }
                for (index, unit) in units.0.iter().enumerate() {
                    if unsafe { std::ptr::read_volatile(local.add(index)) } != *unit {
                        return None;
                    }
                }
                if unsafe { std::ptr::read_volatile(local.add(units.0.len())) } != 0 {
                    return None;
                }
                Some(probe)
            }
        }

        fn read_units(&self, handle: windows_sys::Win32::Foundation::HANDLE) -> Option<Secret> {
            use std::mem::{offset_of, size_of};
            use windows_sys::Win32::Foundation::UNICODE_STRING;
            use windows_sys::Win32::System::Threading::{
                IsWow64Process2, PEB, PROCESS_BASIC_INFORMATION, RTL_USER_PROCESS_PARAMETERS,
            };
            let mut process_machine = 0u16;
            let mut native_machine = 0u16;
            if unsafe { IsWow64Process2(handle, &mut process_machine, &mut native_machine) } == 0
                || process_machine != 0
                || native_machine != 0x8664
            {
                return None;
            }
            let mut basic = PROCESS_BASIC_INFORMATION::default();
            let mut used = 0u32;
            let status = unsafe {
                (self.query)(
                    handle,
                    0,
                    (&mut basic as *mut PROCESS_BASIC_INFORMATION).cast(),
                    size_of::<PROCESS_BASIC_INFORMATION>() as u32,
                    &mut used,
                )
            };
            if status != 0
                || used as usize != size_of::<PROCESS_BASIC_INFORMATION>()
                || basic.PebBaseAddress.is_null()
            {
                return None;
            }
            let pointer_address =
                (basic.PebBaseAddress as usize).checked_add(offset_of!(PEB, ProcessParameters))?;
            let mut parameters = 0usize;
            read_value(handle, pointer_address, &mut parameters)?;
            let descriptor_address =
                parameters.checked_add(offset_of!(RTL_USER_PROCESS_PARAMETERS, CommandLine))?;
            let mut descriptor = UNICODE_STRING::default();
            read_value(handle, descriptor_address, &mut descriptor)?;
            let first = read_command(handle, &descriptor)?;
            let second = read_command(handle, &descriptor)?;
            (first.0 == second.0).then_some(first)
        }
    }

    struct Secret(Vec<u16>);

    impl Drop for Secret {
        fn drop(&mut self) {
            for unit in &mut self.0 {
                unsafe { std::ptr::write_volatile(unit, 0) };
            }
            std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn load_query() -> Option<NtQuery> {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
        let ntdll = unsafe { GetModuleHandleW(windows_sys::core::w!("ntdll.dll")) };
        if ntdll.is_null() {
            return None;
        }
        let symbol =
            unsafe { GetProcAddress(ntdll, c"NtQueryInformationProcess".as_ptr().cast()) }?;
        // SAFETY: `symbol` is ntdll!NtQueryInformationProcess. The signature matches the
        // documented class-0 process-basic-information query used by the existing reader.
        Some(unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, NtQuery>(symbol)
        })
    }

    fn read_process_marker(probe: &CommandProbe, pid: u32) -> Option<(bool, u32)> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
        };
        let handle =
            owned(unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) })
                .ok()?;
        let units = probe.read_units(handle.as_raw_handle())?;
        let parent = parent_of(probe, handle.as_raw_handle()).unwrap_or(0);
        let marker = exception_marker(&units.0);
        Some((marker, parent))
    }

    fn parent_of(
        probe: &CommandProbe,
        handle: windows_sys::Win32::Foundation::HANDLE,
    ) -> Option<u32> {
        use std::mem::size_of;
        use windows_sys::Win32::System::Threading::PROCESS_BASIC_INFORMATION;
        let mut basic = PROCESS_BASIC_INFORMATION::default();
        let mut used = 0u32;
        let status = unsafe {
            (probe.query)(
                handle,
                0,
                (&mut basic as *mut PROCESS_BASIC_INFORMATION).cast(),
                size_of::<PROCESS_BASIC_INFORMATION>() as u32,
                &mut used,
            )
        };
        if status != 0 {
            return None;
        }
        u32::try_from(basic.InheritedFromUniqueProcessId).ok()
    }

    fn exception_marker(units: &[u16]) -> bool {
        contains_units(units, "--harness-cpu-exception-launch")
            || contains_units(units, "cpu-exception-anchor")
            || contains_units(units, "--harness-cpu=uncapped")
            || contains_units(units, "--harness-cpu uncapped")
    }

    fn contains_units(haystack: &[u16], needle: &str) -> bool {
        let needle: Vec<u16> = needle.encode_utf16().collect();
        !needle.is_empty()
            && haystack
                .windows(needle.len())
                .any(|window| window == needle)
    }

    fn read_command(
        handle: windows_sys::Win32::Foundation::HANDLE,
        descriptor: &windows_sys::Win32::Foundation::UNICODE_STRING,
    ) -> Option<Secret> {
        let bytes = usize::from(descriptor.Length);
        if bytes == 0
            || !bytes.is_multiple_of(2)
            || !descriptor.MaximumLength.is_multiple_of(2)
            || descriptor.Length > descriptor.MaximumLength
            || bytes / 2 >= MAX_WIDE
            || !(descriptor.Buffer as usize).is_multiple_of(2)
        {
            return None;
        }
        let mut command = Secret(vec![0; bytes / 2]);
        read_memory(
            handle,
            descriptor.Buffer as usize,
            command.0.as_mut_ptr().cast(),
            bytes,
        )?;
        if command.0.contains(&0) {
            return None;
        }
        Some(command)
    }

    fn read_value<T>(
        handle: windows_sys::Win32::Foundation::HANDLE,
        address: usize,
        value: &mut T,
    ) -> Option<()> {
        read_memory(
            handle,
            address,
            (value as *mut T).cast(),
            std::mem::size_of::<T>(),
        )
    }

    fn read_memory(
        handle: windows_sys::Win32::Foundation::HANDLE,
        address: usize,
        output: *mut std::ffi::c_void,
        bytes: usize,
    ) -> Option<()> {
        use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
        if address == 0 || address.checked_add(bytes).is_none() {
            return None;
        }
        let mut read = 0usize;
        // SAFETY: `output` is a writable local buffer of `bytes`. The remote address is
        // not dereferenced locally, and a short read is rejected.
        (unsafe {
            ReadProcessMemory(
                handle,
                address as *const std::ffi::c_void,
                output,
                bytes,
                &mut read,
            )
        } != 0
            && read == bytes)
            .then_some(())
    }
}

pub use cpu_budget::{CpuBudgetStatus, RouteCoverage, inspect_cpu_budget};

fn disconnected(message: &str) -> io::Error {
    io::Error::other(format!("{message}; preserving it"))
}

fn refuse_pending(codex_home: &Path) -> io::Result<()> {
    for relative in [
        "harness/native-registration/journal.json",
        "harness/native-registration/commit.json",
        "harness/native-registration/complete.json",
        "harness/token-workflow-pending.json",
        "harness/pending.json",
        "AGENTS.override.md",
    ] {
        let path = codex_home.join(relative);
        inventory::ordinary_parents(&path)?;
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            Ok(_) => {
                return Err(disconnected(
                    "pending journal, commit, completion or legacy recovery is present",
                ));
            }
        }
    }
    Ok(())
}

fn read_native(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
) -> io::Result<InstallationMetadata> {
    let path = codex_home.join("harness/installation.json");
    inventory::ordinary_parents(&path)?;
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(disconnected(
                "native schema-2 core installation is not connected",
            ));
        }
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    let snapshot = ConfigSnapshot::read(&path)?;
    let header: serde_json::Value = serde_json::from_slice(snapshot.contents())
        .map_err(|_| disconnected("native core installation metadata is not connected"))?;
    match header.get("schemaVersion").and_then(|value| value.as_u64()) {
        Some(1) => Err(disconnected(
            "legacy installation needs an explicit native upgrade",
        )),
        Some(2) => InstallationMetadata::read(codex_home, user_home, dependency_user_home)?
            .ok_or_else(|| disconnected("native schema-2 core installation is not connected")),
        _ => Err(disconnected(
            "native core installation metadata is not a connected schema-2 record",
        )),
    }
}

fn inspect_links(links: &[InstalledLink]) -> io::Result<()> {
    for link in links {
        inspect_link(link)?;
    }
    Ok(())
}

fn inspect_link(link: &InstalledLink) -> io::Result<()> {
    let directory = matches!(link.object.link_type, LinkType::Directory);
    let guard = match native::verified_link(&link.object.path, &link.object.target, directory) {
        Ok(guard) => guard,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(disconnected("recorded core link is missing"));
        }
        Err(error) => return Err(error),
    };
    if guard.object_identity()? != link.object.identity {
        return Err(disconnected(
            "recorded core link identity, type or target does not match the live object",
        ));
    }
    Ok(())
}

fn inspect_native_launch(
    metadata: &InstallationMetadata,
) -> io::Result<(native_launcher::Registration, ConfigSnapshot)> {
    let link = metadata
        .links()
        .iter()
        .find(|link| link.kind == "native-launch" && link.name == "codex")
        .ok_or_else(|| disconnected("native-launch registration is missing"))?;
    inspect_link(link)?;
    let snapshot = ConfigSnapshot::read(&link.object.target)?;
    let launch: native_launcher::Registration = serde_json::from_slice(snapshot.contents())
        .map_err(|_| disconnected("native-launch registration is invalid"))?;
    if launch.schema != 2 || launch.state.is_some() || launch.build.is_none() {
        return Err(disconnected("native-launch registration is not schema 2"));
    }
    Ok((launch, snapshot))
}

fn require_native_commands(links: &[InstalledLink], build: &Path) -> io::Result<()> {
    let expected: BTreeSet<_> = crate::core_install::core_linked_binaries()
        .map(|name| name.trim_end_matches(".exe").to_owned())
        .collect();
    let mut found = BTreeSet::new();
    for link in links.iter().filter(|link| link.kind == "native-command") {
        if !found.insert(link.name.clone()) {
            return Err(disconnected("native-command records are duplicated"));
        }
        let expected_target = build.join(format!("{}.exe", link.name));
        if !same_path(&link.object.target, &expected_target)? {
            return Err(disconnected(
                "native-command target does not match the registered build",
            ));
        }
    }
    if found != expected {
        return Err(disconnected(
            "native core installation is missing recorded native commands",
        ));
    }
    let diagnostic: Vec<_> = links
        .iter()
        .filter(|link| link.kind == "native-diagnostic")
        .collect();
    if diagnostic.len() != 1
        || diagnostic[0].name != "codex-harness-check"
        || !same_path(
            &diagnostic[0].object.target,
            &build.join("codex-harness.exe"),
        )?
    {
        return Err(disconnected(
            "native diagnostic command is missing or selects another manager",
        ));
    }
    Ok(())
}

fn require_data_links(links: &[InstalledLink], desired: &[inventory::Link]) -> io::Result<()> {
    use crate::installation_state::key;
    for expected in desired {
        let path = key(&expected.destination)?;
        let recorded = links
            .iter()
            .find(|link| key(&link.object.path).ok().as_deref() == Some(path.as_str()))
            .ok_or_else(|| disconnected("current manifest data link is not registered"))?;
        if recorded.kind != expected.kind
            || recorded.name != expected.name
            || !same_path(&recorded.object.target, &expected.source)?
        {
            return Err(disconnected(
                "current manifest data link differs from the installed record",
            ));
        }
    }
    Ok(())
}

fn require_path(scope: PathScope, bin: &Path) -> io::Result<()> {
    let (_, missing) = PathChange::prepend(scope, bin)?;
    if missing {
        return Err(disconnected(match scope {
            PathScope::User => "User PATH does not contain harness/bin",
            PathScope::Process => "Process PATH does not contain harness/bin",
        }));
    }
    Ok(())
}

fn require_current_source(build: &Path, source: &Path) -> io::Result<()> {
    let check = build_identity::check(build, Some(source));
    // Check stays the currency signal even though launches no longer degrade:
    // a stale or unreachable checkout still fails this verification with the
    // deploy action, while damaged or unsupported builds keep their refusal.
    if !check.runtime_allowed {
        return Err(disconnected(
            "current source is unavailable or the native build is not healthy",
        ));
    }
    if check.status != build_identity::Health::Healthy {
        return Err(disconnected(
            "the checkout differs from the delivered build; run explicit deploy to switch new processes to it",
        ));
    }
    let record = build_identity::read_record(build)?;
    if !same_path(&record.source_root, source)? {
        return Err(disconnected(
            "registered source does not match the native build record",
        ));
    }
    Ok(())
}

fn read_instructions(path: &Path) -> io::Result<Vec<u8>> {
    build_identity::ordinary(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err(disconnected(
            "current instruction source is missing or too large",
        ));
    }
    Ok(bytes)
}

fn same_path(left: &Path, right: &Path) -> io::Result<bool> {
    Ok(left.canonicalize()? == right.canonicalize()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        environment_path::UserPathSnapshot,
        installation_metadata::{Previous, Settings, encode},
        installation_state::PathScope,
        inventory::{Connection, Link},
        registration::{Registration, metadata::MetadataDestination},
    };
    use serde_json::json;
    use std::{collections::BTreeMap, path::PathBuf};

    struct Fixture {
        root: PathBuf,
        settings: Settings,
        links: Vec<Link>,
        launch_source: PathBuf,
    }

    impl Fixture {
        fn new(path_scope: PathScope) -> Self {
            let root = tempfile::Builder::new()
                .prefix("native-core-check-")
                .tempdir()
                .unwrap()
                .keep();
            let source = root.join("source");
            let build = root.join("build");
            let home = root.join("codex");
            let user = root.join("user");
            let dependency = root.join("dependency");
            fs::create_dir_all(&source).unwrap();
            fs::create_dir_all(&build).unwrap();
            fs::write(
                source.join("instructions.md"),
                b"Owned native core check.\n",
            )
            .unwrap();
            fs::write(source.join("profile.toml"), b"approval_policy = 'never'\n").unwrap();
            for binary in build_identity::BINARIES {
                fs::write(build.join(binary), binary.as_bytes()).unwrap();
            }
            let upstream = root.join("upstream.exe");
            fs::write(&upstream, b"explicit fixture upstream").unwrap();
            let launch_source = root.join("launch.json");
            fs::write(
                &launch_source,
                serde_json::to_vec_pretty(&native_launcher::Registration {
                    task_control: false,
                    schema: 2,
                    state: None,
                    build: Some(build.clone()),
                    upstream: native_launcher::Upstream {
                        executable: upstream.clone(),
                        sha256: build_identity::hash_file(&upstream).unwrap(),
                        package: None,
                    },
                })
                .unwrap(),
            )
            .unwrap();
            let mut links = vec![
                Link {
                    kind: "instructions".into(),
                    name: "AGENTS".into(),
                    source: source.join("instructions.md"),
                    destination: home.join("AGENTS.md"),
                    connection: Connection::Missing,
                },
                Link {
                    kind: "profile".into(),
                    name: "harness".into(),
                    source: source.join("profile.toml"),
                    destination: home.join("harness.config.toml"),
                    connection: Connection::Missing,
                },
                Link {
                    kind: "native-launch".into(),
                    name: "codex".into(),
                    source: launch_source.clone(),
                    destination: home.join("harness/native-launch.json"),
                    connection: Connection::Missing,
                },
                Link {
                    kind: "native-diagnostic".into(),
                    name: "codex-harness-check".into(),
                    source: build.join("codex-harness.exe"),
                    destination: home.join("harness/bin/codex-harness-check.exe"),
                    connection: Connection::Missing,
                },
            ];
            for binary in crate::core_install::core_linked_binaries() {
                links.push(Link {
                    kind: "native-command".into(),
                    name: binary.trim_end_matches(".exe").into(),
                    source: build.join(binary),
                    destination: home.join("harness/bin").join(binary),
                    connection: Connection::Missing,
                });
            }
            Self {
                settings: Settings {
                    source_root: source,
                    codex_home: home,
                    user_home: user,
                    dependency_user_home: dependency,
                    codex_command: upstream,
                    path_scope,
                    path_added: false,
                    versions: BTreeMap::new(),
                },
                launch_source,
                links,
                root,
            }
        }

        fn metadata_path(&self) -> PathBuf {
            self.settings.codex_home.join("harness/installation.json")
        }

        fn publish(&self, path: bool) {
            let reg = Registration::open(&self.root.join("journal-state")).unwrap();
            let destination = MetadataDestination::absent(&self.metadata_path()).unwrap();
            let settings = self.settings.clone();
            let links = self.links.clone();
            if path {
                let change = UserPathSnapshot::for_registration()
                    .unwrap()
                    .prepend(&self.settings.codex_home.join("harness/bin"))
                    .unwrap()
                    .0;
                reg.apply_installation(
                    &links,
                    &[],
                    &[],
                    &[],
                    destination,
                    |view| encode(&settings, &links, view, Previous::Fresh),
                    Some(change.into()),
                    || Ok(()),
                )
                .unwrap();
            } else {
                reg.apply_with_metadata(&links, &[], &[], &[], destination, |view| {
                    encode(&settings, &links, view, Previous::Fresh)
                })
                .unwrap();
            }
            assert!(reg.finish(&self.metadata_path()).unwrap().committed);
        }

        fn check(&self) -> io::Result<CheckReport> {
            check(
                &self.settings.codex_home,
                &self.settings.user_home,
                &self.settings.dependency_user_home,
                Duration::from_secs(45),
            )
        }
    }

    #[test]
    fn changed_or_unregistered_manifest_data_requires_update() {
        let fixture = Fixture::new(PathScope::User);
        fixture.publish(false);
        let metadata = read_native(
            &fixture.settings.codex_home,
            &fixture.settings.user_home,
            &fixture.settings.dependency_user_home,
        )
        .unwrap();
        require_data_links(metadata.links(), &fixture.links).unwrap();
        let mut changed = fixture.links[0].clone();
        changed.source = fixture.settings.source_root.join("new-instructions.md");
        fs::write(&changed.source, b"new instructions").unwrap();
        assert!(require_data_links(metadata.links(), &[changed]).is_err());
        let mut missing = fixture.links[0].clone();
        missing.destination = fixture.settings.codex_home.join("new-data");
        assert!(require_data_links(metadata.links(), &[missing]).is_err());
        metadata.verify_unchanged().unwrap();
    }

    #[test]
    fn missing_installation_is_not_connected_and_creates_no_homes() {
        let root = tempfile::Builder::new()
            .prefix("native-core-check-missing-")
            .tempdir()
            .unwrap()
            .keep();
        let home = root.join("codex");
        let user = root.join("user");
        let dependency = root.join("dependency");
        let error = check(&home, &user, &dependency, Duration::from_secs(45))
            .err()
            .expect("must reject missing installation");
        assert!(error.to_string().contains("not connected"));
        assert!(!home.exists());
        assert!(!user.exists());
        assert!(!dependency.exists());
    }

    #[test]
    fn pending_recovery_is_refused_before_cli_and_preserved() {
        let fixture = Fixture::new(PathScope::User);
        fixture.publish(false);
        let journal = fixture
            .settings
            .codex_home
            .join("harness/native-registration/journal.json");
        fs::create_dir_all(journal.parent().unwrap()).unwrap();
        fs::write(&journal, b"pending-owned-journal").unwrap();
        let before = fs::read(fixture.metadata_path()).unwrap();
        let error = fixture.check().err().expect("must reject pending state");
        assert!(error.to_string().contains("pending"));
        assert_eq!(fs::read(&journal).unwrap(), b"pending-owned-journal");
        assert_eq!(fs::read(fixture.metadata_path()).unwrap(), before);
    }

    #[test]
    fn legacy_metadata_is_not_connected_and_is_preserved() {
        let root = tempfile::Builder::new()
            .prefix("native-core-check-legacy-")
            .tempdir()
            .unwrap()
            .keep();
        let home = root.join("codex");
        fs::create_dir_all(home.join("harness")).unwrap();
        let metadata = home.join("harness/installation.json");
        let bytes =
            serde_json::to_vec(&json!({"schemaVersion":1,"sourceRoot":root.join("source")}))
                .unwrap();
        fs::write(&metadata, &bytes).unwrap();
        let error = check(
            &home,
            &root.join("user"),
            &root.join("dependency"),
            Duration::from_secs(45),
        )
        .err()
        .expect("must reject legacy state");
        assert!(error.to_string().contains("legacy"));
        assert_eq!(fs::read(&metadata).unwrap(), bytes);
    }

    #[test]
    fn missing_process_path_preserves_metadata() {
        crate::environment_path::with_test_registry(|| {
            let fixture = Fixture::new(PathScope::Process);
            fixture.publish(false);
            let before = fs::read(fixture.metadata_path()).unwrap();
            let error = fixture.check().err().expect("must reject Process PATH");
            assert!(error.to_string().contains("Process PATH"));
            assert_eq!(fs::read(fixture.metadata_path()).unwrap(), before);
        });
    }

    #[test]
    fn user_path_presence_uses_test_registry_not_shell_resolution() {
        crate::environment_path::with_test_registry(|| {
            let fixture = Fixture::new(PathScope::User);
            fixture.publish(false);
            let before = fs::read(fixture.metadata_path()).unwrap();
            let error = fixture.check().err().expect("must reject missing PATH");
            assert!(error.to_string().contains("User PATH"));
            assert!(!error.to_string().contains("alias"));
            assert_eq!(fs::read(fixture.metadata_path()).unwrap(), before);
            let fixture = Fixture::new(PathScope::User);
            fixture.publish(true);
            let after_path = fs::read(fixture.metadata_path()).unwrap();
            let error = fixture.check().err().expect("must reject missing build");
            assert!(!error.to_string().contains("User PATH"));
            assert!(error.to_string().contains("current source"));
            assert_eq!(fs::read(fixture.metadata_path()).unwrap(), after_path);
        });
    }

    #[test]
    fn native_launch_json_must_match_the_original_upstream_hash() {
        crate::environment_path::with_test_registry(|| {
            let fixture = Fixture::new(PathScope::User);
            fixture.publish(false);
            let before = fs::read(fixture.metadata_path()).unwrap();
            let original = fs::read(&fixture.launch_source).unwrap();
            let mut launch: native_launcher::Registration =
                serde_json::from_slice(&original).unwrap();
            launch.upstream.sha256 = "0".repeat(64);
            fs::write(
                &fixture.launch_source,
                serde_json::to_vec_pretty(&launch).unwrap(),
            )
            .unwrap();
            let error = fixture.check().err().expect("must reject altered launch");
            assert!(error.to_string().contains("upstream hash"));
            assert_eq!(fs::read(fixture.metadata_path()).unwrap(), before);
            assert_ne!(fs::read(&fixture.launch_source).unwrap(), original);
        });
    }

    #[test]
    fn recorded_link_absence_is_not_connected() {
        crate::environment_path::with_test_registry(|| {
            let fixture = Fixture::new(PathScope::User);
            fixture.publish(false);
            let before = fs::read(fixture.metadata_path()).unwrap();
            fs::remove_file(fixture.settings.codex_home.join("AGENTS.md")).unwrap();
            let error = fixture.check().err().expect("must reject missing link");
            assert!(
                error.to_string().contains("missing")
                    || error.to_string().contains("changed")
                    || error.to_string().contains("not")
            );
            assert_eq!(fs::read(fixture.metadata_path()).unwrap(), before);
            assert!(!fixture.settings.codex_home.join("AGENTS.md").exists());
        });
    }
}
