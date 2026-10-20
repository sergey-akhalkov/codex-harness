//! One configuration permission check shared by every reflection dispatch path.

use super::*;
use harness_core::delivery_reflection::{LifecycleStatus, ReflectionState};

pub(super) struct ReflectionGate {
    pub(super) enabled: bool,
    pub(super) config_path: PathBuf,
}

impl ReflectionGate {
    /// Re-read the effective kit configuration. A missing key defaults to
    /// true; an invalid value fails here, before any model or lifecycle write.
    pub(super) fn load(source: &Path) -> io::Result<Self> {
        let config = orchestration_config::load(source)?;
        Ok(Self::from_config(source, &config))
    }

    pub(super) fn from_config(source: &Path, config: &orchestration_config::Orchestration) -> Self {
        Self {
            enabled: config.self_reflection_enabled,
            config_path: source.join("global/orchestration.toml"),
        }
    }

    pub(super) fn disabled(&self) -> io::Error {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            format!(
                "self-reflection: model work is disabled by self_reflection_enabled = false in {}; set that key to true to permit a new stage. Retained results, pending stages, usage and recovery were not changed",
                self.config_path.display()
            ),
        )
    }

    pub(super) fn ensure_enabled(&self) -> io::Result<()> {
        if self.enabled {
            Ok(())
        } else {
            Err(self.disabled())
        }
    }

    pub(super) fn model_work(&self, state: Option<&ReflectionState>) -> (&'static str, String) {
        if !self.enabled {
            return (
                "stopped",
                format!(
                    "self_reflection_enabled = false in {}; no new executor or lead reflection model call may start",
                    self.config_path.display()
                ),
            );
        }
        let Some(state) = state else {
            return (
                "stopped",
                "self_reflection_enabled = true, but no reflection lifecycle is active; enable one explicitly"
                    .to_owned(),
            );
        };
        if state.lifecycle != LifecycleStatus::Active {
            return (
                "stopped",
                "self_reflection_enabled = true, but the retained reflection lifecycle is stopped; enable it explicitly"
                    .to_owned(),
            );
        }
        if let Some(recovery) = &state.recovery {
            return (
                "stopped",
                format!(
                    "self_reflection_enabled = true, but reflection recovery is pending: {}",
                    recovery.detail
                ),
            );
        }
        (
            "permitted",
            "self_reflection_enabled = true and the explicitly activated lifecycle is active"
                .to_owned(),
        )
    }
}
