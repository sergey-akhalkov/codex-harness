//! Host-private orchestration records. Interrupted writes stay recoverable.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub schema: u32,
    pub id: String,
    pub workspace: PathBuf,
    pub worktree: Option<PathBuf>,
    pub lead_profile: String,
    pub authorization: String,
    pub requirements: String,
    pub stopped: bool,
    pub active_lead: String,
    pub assignments: Vec<Assignment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignment {
    pub id: String,
    pub profile: String,
    pub owner_thread: Option<String>,
    pub worktree: Option<PathBuf>,
    pub attempt: u32,
    pub accepted: Option<bool>,
    pub defect: Option<String>,
}

pub fn store_dir(codex_home: &Path) -> PathBuf {
    codex_home.join("harness/orchestration")
}

pub fn path(codex_home: &Path, id: &str) -> PathBuf {
    store_dir(codex_home).join(format!("{id}.json"))
}

pub fn save(codex_home: &Path, record: &TaskRecord) -> io::Result<()> {
    let dest = path(codex_home, &record.id);
    fs::create_dir_all(store_dir(codex_home))?;
    let bytes = serde_json::to_vec_pretty(record)?;
    let tmp = dest.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, &dest)?;
    Ok(())
}

pub fn load(codex_home: &Path, id: &str) -> io::Result<TaskRecord> {
    let dest = path(codex_home, id);
    let tmp = dest.with_extension("json.tmp");
    if !dest.is_file() && tmp.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "orchestration record write was interrupted; previous state is recoverable",
        ));
    }
    serde_json::from_slice(&fs::read(&dest)?).map_err(|error| {
        io::Error::new(io::ErrorKind::InvalidData, format!("task record: {error}"))
    })
}

pub fn concurrency_ok(record: &TaskRecord, limit: u32) -> io::Result<()> {
    let active = record
        .assignments
        .iter()
        .filter(|item| item.owner_thread.is_some())
        .count() as u32;
    if active > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("executor concurrency {active} exceeds {limit}"),
        ));
    }
    Ok(())
}

pub fn transfer_lead(record: &mut TaskRecord, successor: &str) {
    record.active_lead = successor.to_owned();
}

pub fn classify_stop(value: &Value) -> bool {
    value["stopped"] == true
}

pub fn accept(record: &mut TaskRecord, assignment: &str) -> io::Result<()> {
    let item = record
        .assignments
        .iter_mut()
        .find(|item| item.id == assignment)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "assignment missing"))?;
    item.accepted = Some(true);
    item.defect = None;
    Ok(())
}

pub fn return_defect(record: &mut TaskRecord, assignment: &str, defect: &str) -> io::Result<()> {
    let item = record
        .assignments
        .iter_mut()
        .find(|item| item.id == assignment)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "assignment missing"))?;
    item.accepted = Some(false);
    item.defect = Some(defect.to_owned());
    if item.owner_thread.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "defect return requires the original executor owner",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, workspace: &Path) -> TaskRecord {
        TaskRecord {
            schema: 1,
            id: id.into(),
            workspace: workspace.to_path_buf(),
            worktree: None,
            lead_profile: "default".into(),
            authorization: "read-only".into(),
            requirements: "synthetic".into(),
            stopped: false,
            active_lead: "default".into(),
            assignments: vec![Assignment {
                id: "a1".into(),
                profile: "xai".into(),
                owner_thread: Some("t1".into()),
                worktree: None,
                attempt: 1,
                accepted: None,
                defect: None,
            }],
        }
    }

    #[test]
    fn interrupted_temp_file_is_recoverable_and_lead_transfer_keeps_assignment() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let workspace = root.path().join("ws");
        let rec = record("task-1", &workspace);
        save(&home, &rec).unwrap();
        let loaded = load(&home, "task-1").unwrap();
        assert_eq!(loaded.authorization, "read-only");
        assert_eq!(loaded.assignments[0].profile, "xai");
        fs::write(path(&home, "task-2").with_extension("json.tmp"), b"{").unwrap();
        let error = load(&home, "task-2").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(error.to_string().contains("recoverable"));
        let mut next = loaded.clone();
        transfer_lead(&mut next, "xai");
        assert_eq!(next.active_lead, "xai");
        assert_eq!(next.assignments[0].id, "a1");
        concurrency_ok(&next, 2).unwrap();
        let error = concurrency_ok(&next, 0).unwrap_err();
        assert!(error.to_string().contains("concurrency"));
        next.stopped = true;
        save(&home, &next).unwrap();
        assert!(classify_stop(&serde_json::json!({"stopped":true})));
        assert!(load(&home, "task-1").unwrap().stopped);
        accept(&mut next, "a1").unwrap();
        assert_eq!(next.assignments[0].accepted, Some(true));
        return_defect(&mut next, "a1", "missing fixture").unwrap();
        assert_eq!(next.assignments[0].accepted, Some(false));
        assert_eq!(next.assignments[0].owner_thread.as_deref(), Some("t1"));
        assert_eq!(
            next.assignments[0].defect.as_deref(),
            Some("missing fixture")
        );
    }
}
