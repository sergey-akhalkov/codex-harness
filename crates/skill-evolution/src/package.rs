//! Canonical identity of one skill package: name, files and content revision.

use crate::{hash_file, invalid, ordinary_metadata};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

const LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub name: String,
    pub description: String,
    pub root: PathBuf,
    pub revision: String,
    pub files: BTreeMap<String, String>,
}

pub fn load(root: &Path) -> io::Result<Identity> {
    let root = root.canonicalize()?;
    let skill = root.join("SKILL.md");
    let metadata = ordinary_metadata(&skill)?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return Err(invalid("invalid SKILL.md"));
    }
    let mut bytes = Vec::new();
    fs::File::open(&skill)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(invalid("SKILL.md exceeds the package limit"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid("SKILL.md is not UTF-8"))?;
    let (name, description) = frontmatter(text)?;
    let files = list_files(&root)?;
    if !files.contains_key("SKILL.md") {
        return Err(invalid("package listing missed SKILL.md"));
    }
    let mut material = name.clone().into_bytes();
    material.push(0);
    for (relative, hash) in &files {
        material.extend_from_slice(relative.as_bytes());
        material.push(0);
        material.extend_from_slice(hash.as_bytes());
        material.push(0);
    }
    Ok(Identity {
        name,
        description,
        root,
        revision: crate::hash_bytes(&material),
        files,
    })
}

pub fn copy_into(source: &Path, destination: &Path) -> io::Result<Identity> {
    let source = load(source)?;
    fs::create_dir_all(destination)?;
    for relative in source.files.keys() {
        let from = source.root.join(relative);
        let to = destination.join(relative);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut input = fs::File::open(&from)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&to)?;
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
    }
    let copied = load(destination)?;
    if copied.name != source.name
        || copied.description != source.description
        || copied.files != source.files
        || copied.revision != source.revision
    {
        return Err(invalid("copied skill package does not match the source"));
    }
    Ok(copied)
}

fn list_files(root: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = ordinary_metadata(&path)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid("package walk escaped the root"))?
                .to_str()
                .ok_or_else(|| invalid("package path is not UTF-8"))?
                .replace('\\', "/");
            if metadata.is_dir() {
                if relative
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.starts_with('.'))
                {
                    continue;
                }
                pending.push(path);
                continue;
            }
            if !metadata.is_file() || metadata.len() > LIMIT {
                return Err(invalid("skill package contains a non-ordinary file"));
            }
            files.insert(relative, hash_file(&path)?);
        }
    }
    Ok(files)
}

fn frontmatter(text: &str) -> io::Result<(String, String)> {
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return Err(invalid("SKILL.md is missing frontmatter"));
    }
    let mut name = None;
    let mut description = None;
    for line in lines.by_ref() {
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("name:") {
            name = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("description:") {
            description = Some(value.trim().to_owned());
        }
    }
    match (name, description) {
        (Some(name), Some(description))
            if !name.is_empty() && !description.is_empty() && name.len() <= 128 =>
        {
            Ok((name, description))
        }
        _ => Err(invalid("SKILL.md frontmatter lacks name or description")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn revision_covers_every_file_and_copy_is_byte_identical() {
        let root = tempfile::tempdir().unwrap();
        let skill = root.path().join("skill");
        fs::create_dir_all(skill.join("references")).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo\ndescription: Owned package fixture.\n---\nBody\n",
        )
        .unwrap();
        fs::write(skill.join("references/note.md"), "note").unwrap();
        let loaded = load(&skill).unwrap();
        assert_eq!(loaded.name, "demo");
        assert_eq!(loaded.files.len(), 2);
        let dest = root.path().join("copy");
        let copied = copy_into(&skill, &dest).unwrap();
        assert_eq!(copied.revision, loaded.revision);
        fs::write(skill.join("references/note.md"), "changed").unwrap();
        assert_ne!(load(&skill).unwrap().revision, loaded.revision);
    }
}
