//! Installer-only preparation of ordinary metadata from guarded candidate IDs.
//! Bytes belong to the same registration journal; this defines no installation format.
// The semantic installer consumer is implemented separately from this bridge.
#![cfg_attr(not(test), allow(dead_code))]

use super::*;
use crate::config_file::ConfigSnapshot;
use crate::installation_metadata::InstallationOwners;

pub(super) fn validate_owners(journal: &Journal, owners: &InstallationOwners) -> io::Result<()> {
    let path = owners.metadata_path();
    let mut candidates = Vec::new();
    for config in &journal.configurations {
        if config.path == path {
            candidates.push((config.published_bytes(), config.published_identity()));
        }
    }
    for creation in &journal.creations {
        if creation.path == path {
            candidates.push((creation.published_bytes(), creation.published_identity()));
        }
    }
    if candidates.len() != 1 {
        return Err(invalid(
            "recovery does not bind exactly one native installation metadata record",
        ));
    }
    owners.validate_candidate(candidates[0].0, candidates[0].1)
}

/// An explicit absence claim or an exact ordinary-file snapshot. No adoption.
pub(crate) enum MetadataDestination {
    Absent(ConfigCreation),
    Existing(ConfigRecord),
}

impl MetadataDestination {
    pub(crate) fn absent(path: &Path) -> io::Result<Self> {
        Ok(Self::Absent(ConfigCreation::new(path, &[])?))
    }

    pub(crate) fn existing(snapshot: &ConfigSnapshot) -> io::Result<Self> {
        Ok(Self::Existing(
            snapshot.plan_replace(snapshot.contents())?.record,
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MetadataLink {
    pub path: PathBuf,
    pub target: PathBuf,
    pub link_type: LinkType,
    pub identity: LinkIdentity,
}

/// Immutable input only. Removed links occur only in `previous`; replacements
/// have their old ID there and their newly staged ID in `links`. Reused links
/// occur in both. Capture is not ownership: compare `previous` to prior metadata
/// or an explicitly authorized legacy claim before returning any bytes.
pub(crate) struct MetadataView {
    pub links: Vec<MetadataLink>,
    pub previous: Vec<MetadataLink>,
    pub path: PathBuf,
    pub identity: LinkIdentity,
    /// Whole original file hash from the already held exact snapshot; None is
    /// reserved absence. Consumers can bind prior semantics without reopening a
    /// metadata file whose write/delete handles are intentionally excluded.
    pub previous_metadata_sha256: Option<String>,
}

type BuildBytes<'a> = Box<dyn FnOnce(&MetadataView) -> io::Result<Vec<u8>> + 'a>;

pub(super) struct Builder<'a> {
    destination: MetadataDestination,
    build: BuildBytes<'a>,
}

pub(super) struct Activation<'a> {
    pub path_change: Option<crate::environment_path::UserPathChange>,
    pub process_path: Option<crate::process_path::ProcessPathChange>,
    pub retire_configuration: Option<PathBuf>,
    pub verify: Box<dyn FnOnce() -> io::Result<()> + 'a>,
}

#[derive(Clone, Copy)]
pub(super) enum Slot {
    Creation(usize),
    Configuration(usize),
}

impl Registration {
    pub(crate) fn recover_owned_installation(
        &self,
        owners: &InstallationOwners,
    ) -> io::Result<UndoReport> {
        let owner_guard = FileGuard::open_regular(&self.state.join("owner"), OWNER)?;
        if let Some(report) = finish::resume_checked(self, &owner_guard, |journal| {
            validate_owners(journal, owners)
        })? {
            return Ok(report);
        }
        match self.load_journal()? {
            Some(snapshot) if snapshot.completion.is_some() => {
                drop(owner_guard);
                self.finish_owned_installation(owners)
            }
            Some(snapshot) => {
                // undo opens the same bytes under its journal guard before any
                // PATH/file inverse; a replacement after validation is refused.
                validate_owners(&snapshot.journal, owners)?;
                self.undo_checked(snapshot, false, &owner_guard)
            }
            None => Ok(UndoReport {
                removed: Vec::new(),
                restored: Vec::new(),
                committed: false,
            }),
        }
    }

    /// Completion is written only after installer acceptance. Resume committed
    /// cleanup or finish that accepted candidate; otherwise undo partial apply.
    pub(crate) fn recover_installation(&self, metadata: &Path) -> io::Result<UndoReport> {
        if self.state.join(finish::COMMIT).try_exists()? {
            return self.recover();
        }
        match self.load_journal()? {
            Some(snapshot) if snapshot.completion.is_some() => self.finish(metadata),
            _ => self.recover(),
        }
    }

    /// Inject an owned failure after metadata publication without adding a
    /// production switch or altering the ordinary registration entry point.
    #[cfg(test)]
    pub(crate) fn test_apply_metadata(
        &self,
        links: &[Link],
        destination: MetadataDestination,
        build: impl FnOnce(&MetadataView) -> io::Result<Vec<u8>>,
        checkpoint: impl FnMut() -> io::Result<()>,
    ) -> io::Result<ApplyReport> {
        self.apply_prepared(
            links,
            &[],
            &[],
            &[],
            Some(Builder {
                destination,
                build: Box::new(build),
            }),
            None,
            checkpoint,
        )
    }

    /// Prepares bytes only, with all input IDs guarded and every new object
    /// still invisible. The builder must not publish data or include its own
    /// content hash/journal digest. Finish supplies the external exact witness.
    /// A pending journal must be finished or explicitly undone before rebuilding.
    pub(crate) fn apply_with_metadata(
        &self,
        links: &[Link],
        configurations: &[ConfigChange],
        creations: &[ConfigCreation],
        link_changes: &[LinkChange],
        destination: MetadataDestination,
        build: impl FnOnce(&MetadataView) -> io::Result<Vec<u8>>,
    ) -> io::Result<ApplyReport> {
        self.apply_prepared(
            links,
            configurations,
            creations,
            link_changes,
            Some(Builder {
                destination,
                build: Box::new(build),
            }),
            None,
            || Ok(()),
        )
    }

    /// Installer activation keeps PATH and runtime acceptance under the same
    /// immutable intent. A failure before completion remains recoverable.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_installation(
        &self,
        links: &[Link],
        configurations: &[ConfigChange],
        creations: &[ConfigCreation],
        link_changes: &[LinkChange],
        destination: MetadataDestination,
        build: impl FnOnce(&MetadataView) -> io::Result<Vec<u8>>,
        path_change: Option<crate::installation_path::PathChange>,
        verify: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<ApplyReport> {
        let (path_change, process_path) =
            path_change.map(|change| change.split()).unwrap_or_default();
        self.apply_prepared(
            links,
            configurations,
            creations,
            link_changes,
            Some(Builder {
                destination,
                build: Box::new(build),
            }),
            Some(Activation {
                path_change,
                process_path,
                retire_configuration: None,
                verify: Box::new(verify),
            }),
            || Ok(()),
        )
    }

    /// Keep the exact installer metadata as the recovery witness until an
    /// irreversible decision accepts disconnection, then retire that same file.
    pub(crate) fn apply_disconnection(
        &self,
        changes: &[LinkChange],
        snapshot: &ConfigSnapshot,
        build: impl FnOnce(&MetadataView) -> io::Result<Vec<u8>>,
        path_change: Option<crate::installation_path::PathChange>,
        verify: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<ApplyReport> {
        let (path_change, process_path) =
            path_change.map(|change| change.split()).unwrap_or_default();
        self.apply_prepared(
            &[],
            &[],
            &[],
            changes,
            Some(Builder {
                destination: MetadataDestination::existing(snapshot)?,
                build: Box::new(build),
            }),
            Some(Activation {
                path_change,
                process_path,
                retire_configuration: Some(snapshot.path().to_owned()),
                verify: Box::new(verify),
            }),
            || Ok(()),
        )
    }
}

impl Builder<'_> {
    pub(super) fn baseline_sha256(&self) -> Option<String> {
        match &self.destination {
            MetadataDestination::Absent(_) => None,
            MetadataDestination::Existing(record) => Some(record.baseline_sha256()),
        }
    }

    /// Placeholders reserve paths and snapshot identities in the original
    /// preflight. Only their candidate bytes can change after staging.
    pub(super) fn reserve(
        &self,
        configurations: &mut Vec<ConfigRecord>,
        creations: &mut Vec<ConfigCreation>,
    ) -> Slot {
        match &self.destination {
            MetadataDestination::Absent(plan) => {
                let index = creations.len();
                creations.push(plan.clone());
                Slot::Creation(index)
            }
            MetadataDestination::Existing(record) => {
                let index = configurations.len();
                configurations.push(record.clone());
                Slot::Configuration(index)
            }
        }
    }

    pub(super) fn build(self, view: &MetadataView) -> io::Result<Vec<u8>> {
        // Callback diagnostics can contain private candidate data.
        let bytes = (self.build)(view)
            .map_err(|_| invalid("registration metadata builder failed; no changes published"))?;
        if bytes.len() > MAX_JOURNAL as usize {
            return Err(invalid(
                "registration metadata exceeds the journal size limit",
            ));
        }
        // Encoding the complete journal later also enforces its aggregate bound.
        Ok(bytes)
    }
}

pub(super) fn guarded(path: &Path, guard: &FileGuard) -> io::Result<MetadataLink> {
    let (target, directory) = guard.link_description()?;
    Ok(MetadataLink {
        path: path.to_owned(),
        target,
        link_type: if directory {
            LinkType::Directory
        } else {
            LinkType::File
        },
        identity: guard.object_identity()?,
    })
}

pub(super) fn staged(path: &Path, stage: &StagedLink) -> io::Result<MetadataLink> {
    let (target, directory) = stage.link_description()?;
    Ok(MetadataLink {
        path: path.to_owned(),
        target,
        link_type: if directory {
            LinkType::Directory
        } else {
            LinkType::File
        },
        identity: stage.identity(),
    })
}

#[cfg(test)]
#[path = "../tests/registration_metadata/cases.rs"]
mod tests;
