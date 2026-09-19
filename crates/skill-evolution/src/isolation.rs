//! Host-frozen control files stay outside the writable case and are probed
//! before any model run. Isolation does not launch Codex.

use crate::{
    hash_file, invalid, ordinary_metadata,
    package::{self, Identity},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub source_root: PathBuf,
    pub case_root: PathBuf,
    pub control_root: PathBuf,
    pub library_root: PathBuf,
    pub session_marker: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub isolation_verified: bool,
    pub model_calls: u32,
    pub library: Identity,
    pub control_hashes: BTreeMap<String, String>,
    pub session_marker_sha256: String,
    pub source_marker_unchanged: bool,
    pub control_files_write: String,
    pub control_create_child: String,
    pub case_outside_source: bool,
    pub control_outside_case: bool,
}

pub fn isolate(request: &Request) -> io::Result<Report> {
    let source = canonicalize_dir(&request.source_root)?;
    let case = isolated_dir(&request.case_root, &source)?;
    let control = isolated_dir(&request.control_root, &source)?;
    let library = isolated_dir(&request.library_root, &source)?;
    if overlaps(&case, &control) || overlaps(&case, &library) || overlaps(&control, &library) {
        return Err(invalid("case, control and library roots must be disjoint"));
    }
    let marker = request.session_marker.canonicalize()?;
    if !ordinary_metadata(&marker)?.is_file() {
        return Err(invalid("session marker must be an ordinary file"));
    }
    let before_marker = hash_file(&marker)?;
    let identity = package::load(&library)?;
    let control_hashes = freeze_control(&control)?;
    if control_hashes.is_empty() {
        return Err(invalid("control root has no oracle or baseline files"));
    }
    let control_files_write = probe_write(&control.join("oracle.json"))?;
    let control_create_child = probe_create(&control.join("injected-by-candidate.json"))?;
    let after_marker = hash_file(&marker)?;
    let source_unchanged = before_marker == after_marker;
    let isolation_verified = control_files_write == "denied"
        && source_unchanged
        && !case.starts_with(&source)
        && !control.starts_with(&case);
    Ok(Report {
        isolation_verified,
        model_calls: 0,
        library: identity,
        control_hashes,
        session_marker_sha256: after_marker,
        source_marker_unchanged: source_unchanged,
        control_files_write,
        control_create_child,
        case_outside_source: !case.starts_with(&source),
        control_outside_case: !control.starts_with(&case),
    })
}

fn freeze_control(root: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut hashes = BTreeMap::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = ordinary_metadata(&path)?;
        if !metadata.is_file() {
            return Err(invalid("control root must contain only ordinary files"));
        }
        let mut permissions = metadata.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid("control file name is not UTF-8"))?;
        hashes.insert(name.to_owned(), hash_file(&path)?);
    }
    Ok(hashes)
}

fn probe_write(path: &Path) -> io::Result<String> {
    match OpenOptions::new().write(true).open(path) {
        Ok(_) => Ok("allowed".into()),
        Err(error) if error.kind() == ErrorKind::PermissionDenied => Ok("denied".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Err(invalid("control oracle.json is missing"))
        }
        Err(error) => Err(error),
    }
}

fn probe_create(path: &Path) -> io::Result<String> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(_) => {
            fs::remove_file(path)?;
            Ok("allowed".into())
        }
        Err(error) if error.kind() == ErrorKind::PermissionDenied => Ok("denied".into()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok("allowed".into()),
        Err(error) => Err(error),
    }
}

fn isolated_dir(path: &Path, source: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid("isolation paths must be absolute"));
    }
    let path = path.canonicalize()?;
    let temp = std::env::temp_dir().canonicalize()?;
    if !path.is_dir() || path == temp || !path.starts_with(&temp) || path.starts_with(source) {
        return Err(invalid(
            "case, control and library must be temporary directories outside source",
        ));
    }
    Ok(path)
}

fn canonicalize_dir(path: &Path) -> io::Result<PathBuf> {
    let path = path.canonicalize()?;
    if !path.is_dir() {
        return Err(invalid("source_root must be a directory"));
    }
    Ok(path)
}

fn overlaps(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_skill(root: &Path) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nname: project-verification\ndescription: Owned isolation fixture.\n---\nBody\n",
        )
        .unwrap();
    }

    #[test]
    fn control_files_are_not_writable_and_source_marker_stays_put() {
        let source = tempfile::tempdir().unwrap();
        let marker = source.path().join("live-skill.md");
        fs::write(&marker, "live").unwrap();
        let temp = tempfile::Builder::new()
            .prefix("skill-evolution-iso-")
            .tempdir()
            .unwrap();
        let case = temp.path().join("case");
        let control = temp.path().join("control");
        let library = temp.path().join("library");
        fs::create_dir(&case).unwrap();
        fs::create_dir(&control).unwrap();
        write_skill(&library);
        fs::write(control.join("oracle.json"), "{\"pass\":true}").unwrap();
        fs::write(control.join("baseline.json"), "{\"revision\":\"L\"}").unwrap();
        let before = fs::read(&marker).unwrap();
        let report = isolate(&Request {
            source_root: source.path().to_path_buf(),
            case_root: case,
            control_root: control.clone(),
            library_root: library,
            session_marker: marker.clone(),
        })
        .unwrap();
        assert!(report.isolation_verified);
        assert_eq!(report.model_calls, 0);
        assert_eq!(report.control_files_write, "denied");
        assert_eq!(report.library.name, "project-verification");
        assert_eq!(fs::read(&marker).unwrap(), before);
        assert!(
            OpenOptions::new()
                .write(true)
                .open(control.join("oracle.json"))
                .is_err()
        );
    }

    #[test]
    fn control_inside_the_case_is_rejected() {
        let source = tempfile::tempdir().unwrap();
        let marker = source.path().join("marker.txt");
        fs::write(&marker, "x").unwrap();
        let temp = tempfile::Builder::new()
            .prefix("skill-evolution-bad-")
            .tempdir()
            .unwrap();
        let case = temp.path().join("case");
        fs::create_dir(&case).unwrap();
        let control = case.join("control");
        fs::create_dir(&control).unwrap();
        fs::write(control.join("oracle.json"), "{}").unwrap();
        let library = temp.path().join("library");
        write_skill(&library);
        assert!(
            isolate(&Request {
                source_root: source.path().to_path_buf(),
                case_root: case,
                control_root: control,
                library_root: library,
                session_marker: marker,
            })
            .is_err()
        );
    }
}
