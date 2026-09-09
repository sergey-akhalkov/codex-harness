//! Prepare native feature edits in an owned home, without touching the caller's
//! configuration. Explicit publication uses an existing configuration snapshot;
//! a multi-step installation still needs its own journal and recovery protocol.
#![cfg(windows)]

use crate::config_file::{ConfigChange, ConfigSnapshot};
use crate::process::{CommandSpec, StopReason};
use crate::{build_identity, native_build};
use serde_json::json;
use std::{
    fmt, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const CONFIG_LIMIT: usize = 1024 * 1024;
const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub enum Feature {
    Hooks,
    CodeMode,
}

impl Feature {
    fn name(self) -> &'static str {
        match self {
            Self::Hooks => "hooks",
            Self::CodeMode => "code_mode",
        }
    }
}

pub struct FeatureEdit {
    pub before_sha256: String,
    pub after_sha256: String,
    pub evidence: PathBuf,
    before: Vec<u8>,
    proposed: Vec<u8>,
}

#[derive(Debug)]
pub struct FeatureObservation {
    pub hooks: bool,
    pub code_mode: bool,
    pub evidence: PathBuf,
}

impl FeatureEdit {
    pub fn proposed_config(&self) -> &[u8] {
        &self.proposed
    }

    /// Publishes only to a snapshot matching this preparation's exact input.
    /// Editable public receipt hashes are never used as publication authority.
    pub fn publish_to(&self, baseline: &ConfigSnapshot) -> io::Result<ConfigSnapshot> {
        self.plan_for(baseline)?.publish()
    }

    /// Binds an accepted candidate to its original file snapshot for inclusion
    /// in a native installation journal before any publication occurs.
    pub fn plan_for(&self, baseline: &ConfigSnapshot) -> io::Result<ConfigChange> {
        if baseline.contents() != self.before {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Feature preparation input does not match the configuration snapshot.",
            ));
        }
        baseline.plan_replace(&self.proposed)
    }
}

impl fmt::Debug for FeatureEdit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FeatureEdit")
            .field("before_sha256", &self.before_sha256)
            .field("after_sha256", &self.after_sha256)
            .field("evidence", &self.evidence)
            .finish_non_exhaustive()
    }
}

fn bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::other("Native feature output exceeds its bound."));
    }
    Ok(bytes)
}

fn effective(output: &[u8], feature: Feature, enabled: bool) -> bool {
    let Ok(output) = std::str::from_utf8(output) else {
        return false;
    };
    let mut matching = output.lines().filter_map(|line| {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        (fields.first() == Some(&feature.name())).then_some(fields)
    });
    let Some(fields) = matching.next() else {
        return false;
    };
    fields.len() >= 3
        && fields.last() == Some(&if enabled { "true" } else { "false" })
        && matching.next().is_none()
}

fn observed(output: &[u8], feature: Feature) -> io::Result<bool> {
    match (
        effective(output, feature, true),
        effective(output, feature, false),
    ) {
        (true, false) => Ok(true),
        (false, true) => Ok(false),
        _ => Err(io::Error::other(
            "Native feature discovery returned incomplete or ambiguous coverage.",
        )),
    }
}

/// Observes the selected real home through `features list`. No edit command,
/// shell, PATH discovery, authentication copy or model request is used. Private
/// process output is retained separately; base-config changes invalidate success.
pub fn discover_features(
    upstream: &Path,
    home: &Path,
    timeout: Duration,
) -> io::Result<FeatureObservation> {
    if !upstream.is_absolute()
        || !upstream
            .extension()
            .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
        || !home.is_absolute()
        || timeout.is_zero()
        || Instant::now().checked_add(timeout).is_none()
    {
        return Err(io::Error::other(
            "Absolute native executable/home and finite timeout are required.",
        ));
    }
    crate::inventory::ordinary_parents(&home.join("config.toml"))?;
    if !fs::metadata(home)?.is_dir() {
        return Err(io::Error::other(
            "Native feature home must be an existing ordinary directory.",
        ));
    }
    let config = home.join("config.toml");
    let baseline = match ConfigSnapshot::read(&config) {
        Ok(snapshot) => {
            if snapshot.contents().len() > CONFIG_LIMIT {
                return Err(io::Error::other(
                    "Native configuration exceeds its size bound.",
                ));
            }
            Some(snapshot.plan_replace(snapshot.contents())?)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    build_identity::ordinary(upstream)?;
    let upstream_hash = build_identity::hash_file(upstream)?;
    let root = tempfile::Builder::new()
        .prefix("harness-native-feature-discovery-")
        .tempdir()?
        .keep();
    let result = (|| {
        let mut command = CommandSpec::new(upstream);
        command.args = ["features", "list"].into_iter().map(Into::into).collect();
        // Keep unrelated caller project configuration outside this observation.
        command.current_dir = Some(root.clone());
        command
            .env
            .insert("CODEX_HOME".into(), Some(home.as_os_str().to_owned()));
        let outcome = native_build::invoke_management(
            command,
            &root.join("stderr.txt"),
            Some(&root.join("stdout.txt")),
            timeout,
        )?;
        fs::write(
            root.join("process.json"),
            serde_json::to_vec_pretty(&outcome)?,
        )?;
        if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
            return Err(io::Error::other(
                "Native feature discovery command did not succeed.",
            ));
        }
        let output = bounded(&root.join("stdout.txt"), OUTPUT_LIMIT)?;
        let hooks = observed(&output, Feature::Hooks)?;
        let code_mode = observed(&output, Feature::CodeMode)?;
        if let Some(baseline) = &baseline {
            baseline.record.check_before()?;
        } else if !fs::symlink_metadata(&config).is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
        {
            return Err(io::Error::other(
                "Native configuration appeared during discovery; result not accepted.",
            ));
        }
        if build_identity::hash_file(upstream)? != upstream_hash {
            return Err(io::Error::other(
                "Native executable changed during feature discovery.",
            ));
        }
        fs::write(
            root.join("receipt.json"),
            serde_json::to_vec_pretty(&json!({
                "schema":1,"passed":true,"upstream":upstream,"upstream_sha256":upstream_hash,
                "codex_home":home,"hooks":hooks,"code_mode":code_mode,"base_configuration_unchanged":true
            }))?,
        )?;
        Ok(FeatureObservation {
            hooks,
            code_mode,
            evidence: root.clone(),
        })
    })();
    result.map_err(|error: io::Error| {
        io::Error::new(
            error.kind(),
            format!(
                "Native feature discovery failed: {error} Private evidence: {}",
                root.display()
            ),
        )
    })
}

/// Uses the caller-selected original native Codex executable. No PATH lookup,
/// shell host, authentication copy, live home write or model request is involved.
/// All private intermediate files remain in `evidence`, including failed runs.
pub fn prepare_feature(
    upstream: &Path,
    before: &[u8],
    feature: Feature,
    enabled: bool,
    timeout: Duration,
) -> io::Result<FeatureEdit> {
    if before.len() > CONFIG_LIMIT || std::str::from_utf8(before).is_err() {
        return Err(io::Error::other(
            "Invalid native configuration size or encoding.",
        ));
    }
    if !upstream.is_absolute()
        || !upstream
            .extension()
            .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
        || timeout.is_zero()
    {
        return Err(io::Error::other(
            "An absolute original native executable and finite timeout are required.",
        ));
    }
    build_identity::ordinary(upstream)?;
    let upstream_hash = build_identity::hash_file(upstream)?;
    let until = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::other("Native feature deadline overflow."))?;
    let root = tempfile::Builder::new()
        .prefix("harness-native-feature-")
        .tempdir()?
        .keep();
    let home = root.join("home");
    fs::create_dir(&home)?;
    fs::write(home.join("config.toml"), before)?;
    let result = (|| {
        for (step, args) in [
            (
                "edit",
                vec![
                    "features",
                    if enabled { "enable" } else { "disable" },
                    feature.name(),
                ],
            ),
            ("observe", vec!["features", "list"]),
        ] {
            let mut command = CommandSpec::new(upstream);
            command.args = args.into_iter().map(Into::into).collect();
            command.current_dir = Some(home.clone());
            command
                .env
                .insert("CODEX_HOME".into(), Some(home.clone().into_os_string()));
            let remaining = until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::other("Native feature preparation timed out."));
            }
            let outcome = native_build::invoke_management(
                command,
                &root.join(format!("{step}.stderr.txt")),
                Some(&root.join(format!("{step}.stdout.txt"))),
                remaining,
            )?;
            fs::write(
                root.join(format!("{step}.process.json")),
                serde_json::to_vec_pretty(&outcome)?,
            )?;
            if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
                return Err(io::Error::other("Native feature command did not succeed."));
            }
        }
        if !effective(
            &bounded(&root.join("observe.stdout.txt"), OUTPUT_LIMIT)?,
            feature,
            enabled,
        ) {
            return Err(io::Error::other(
                "Native feature observation did not confirm the requested value.",
            ));
        }
        if build_identity::hash_file(upstream)? != upstream_hash {
            return Err(io::Error::other(
                "Native executable changed during feature preparation.",
            ));
        }
        let proposed = bounded(&home.join("config.toml"), CONFIG_LIMIT)?;
        let edit = FeatureEdit {
            before_sha256: build_identity::hash_bytes(before),
            after_sha256: build_identity::hash_bytes(&proposed),
            evidence: root.clone(),
            before: before.to_owned(),
            proposed,
        };
        fs::write(
            root.join("receipt.json"),
            serde_json::to_vec_pretty(&json!({
                "schema":1,"passed":true,"feature":feature.name(),"enabled":enabled,
                "upstream":upstream,"upstream_sha256":upstream_hash,
                "before_sha256":edit.before_sha256,"after_sha256":edit.after_sha256,
                "live_configuration_modified":false
            }))?,
        )?;
        Ok(edit)
    })();
    result.map_err(|error: io::Error| {
        // These errors describe process/state failures; raw native stdout and
        // stderr (which can quote private TOML) remain in the owned evidence.
        io::Error::new(
            error.kind(),
            format!(
                "Native feature preparation failed: {error} Private evidence: {}",
                root.display()
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_state_requires_one_exact_complete_feature_row() {
        assert!(effective(
            b"hooks stable false\ncode_mode stable true\n",
            Feature::Hooks,
            false
        ));
        assert!(effective(
            b"code_mode under development true\n",
            Feature::CodeMode,
            true
        ));
        for text in [
            "",
            "other_hooks stable false",
            "hooks false",
            "hooks stable true",
            "hooks stable false extra",
            "hooks stable false\nhooks stable true",
            "error mentions hooks stable false",
        ] {
            assert!(!effective(text.as_bytes(), Feature::Hooks, false), "{text}");
        }
        assert!(!effective(&[255], Feature::Hooks, false));
        assert!(
            !observed(
                b"hooks stable false\ncode_mode under development true\n",
                Feature::Hooks
            )
            .unwrap()
        );
        assert!(
            observed(
                b"hooks stable false\ncode_mode under development true\n",
                Feature::CodeMode
            )
            .unwrap()
        );
        for body in [
            b"hooks stable false\nhooks stable false\n".as_slice(),
            b"hooks stable unknown",
            b"hooks false",
            b"code_mode stable true",
        ] {
            assert!(observed(body, Feature::Hooks).is_err());
        }
    }

    #[test]
    fn invalid_inputs_never_launch_or_echo_configuration() {
        let secret = b"PRIVATE_FEATURE_SENTINEL";
        for timeout in [Duration::ZERO, Duration::MAX] {
            let error = discover_features(
                Path::new("C:/owned/codex.exe"),
                Path::new("C:/owned/home"),
                timeout,
            )
            .unwrap_err();
            assert!(error.to_string().contains("finite timeout"));
        }
        for path in [Path::new("codex.exe"), Path::new("C:/abs/codex.ps1")] {
            let error =
                prepare_feature(path, secret, Feature::Hooks, false, Duration::from_secs(1))
                    .unwrap_err();
            assert!(!error.to_string().contains("PRIVATE_FEATURE_SENTINEL"));
        }
        assert!(
            prepare_feature(
                Path::new("C:/abs/codex.exe"),
                &[255],
                Feature::Hooks,
                false,
                Duration::from_secs(1)
            )
            .is_err()
        );
        assert!(
            prepare_feature(
                Path::new("C:/abs/codex.exe"),
                &vec![b' '; CONFIG_LIMIT + 1],
                Feature::Hooks,
                false,
                Duration::from_secs(1)
            )
            .is_err()
        );
    }
}
