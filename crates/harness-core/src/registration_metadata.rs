//! Installer-only preparation of ordinary metadata from guarded candidate IDs.
//! Bytes belong to the same schema-8 journal; this defines no installation format.
// The semantic installer consumer is implemented separately from this bridge.
#![cfg_attr(not(test), allow(dead_code))]

use super::*;
use crate::config_file::ConfigSnapshot;

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

#[derive(Clone, Copy)]
pub(super) enum Slot {
    Creation(usize),
    Configuration(usize),
}

impl Registration {
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
