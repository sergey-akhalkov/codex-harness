//! Model-free working-tree audit. Private audit inputs never belong in the pack.
use regex::Regex;
use std::{collections::BTreeSet, env, fs, path::Path, process::Command};

fn visible(path: &str, terms: &[String]) -> String {
    if terms.iter().any(|term| path.to_lowercase().contains(term)) {
        "[private-path]".into()
    } else {
        path.chars().flat_map(char::escape_default).collect()
    }
}

fn decoded(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            result.push(byte);
            i += 3;
            continue;
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

fn prose(source: &str) -> Vec<(usize, &str)> {
    let mut fence: Option<&str> = None;
    source
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let text = line.trim_start();
            for marker in ["```", "~~~"] {
                if text.starts_with(marker) {
                    if fence == Some(marker) {
                        fence = None;
                    } else if fence.is_none() {
                        fence = Some(marker);
                    }
                    return None;
                }
            }
            fence.is_none().then_some((i + 1, line))
        })
        .collect()
}

fn has_anchor(source: &str, wanted: &str) -> bool {
    let mut slugs = BTreeSet::new();
    for (_, line) in prose(source) {
        let text = line.trim_start();
        if text.starts_with('#') {
            let heading = text
                .trim_start_matches('#')
                .trim()
                .trim_end_matches('#')
                .trim();
            let slug: String = heading
                .to_lowercase()
                .chars()
                .filter_map(|c| {
                    if c.is_whitespace() {
                        Some('-')
                    } else if c.is_alphanumeric() || c == '-' || c == '_' {
                        Some(c)
                    } else {
                        None
                    }
                })
                .collect();
            let mut unique = slug.clone();
            let mut suffix = 0;
            while slugs.contains(&unique) {
                suffix += 1;
                unique = format!("{slug}-{suffix}");
            }
            slugs.insert(unique);
        }
        if text.contains(&format!("id=\"{wanted}\""))
            || text.contains(&format!("name=\"{wanted}\""))
            || text.contains(&format!("id='{wanted}'"))
        {
            return true;
        }
    }
    slugs.contains(wanted)
}

fn run() -> Result<bool, &'static str> {
    let mut args = env::args().skip(1);
    let mut root = env::current_dir().map_err(|_| "working directory unavailable")?;
    let mut terms_path = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = args.next().ok_or("--root requires a directory")?.into(),
            "--private-terms" => {
                terms_path = Some(args.next().ok_or("--private-terms requires a local file")?)
            }
            "--help" | "-h" => {
                println!(
                    "harness-source-check [--root REPOSITORY] [--private-terms LOCAL_FILE]\nChecks existing tracked and non-ignored new files, caches, shared trust settings,\nmachine home paths in documentation/configuration, caller-supplied private terms,\nand local inline Markdown file/heading links. Skips fenced examples, template\ntargets and non-text assets. Terms are one per line in a file outside the repository.\nDiagnostics omit matched text. Exit: 0 clean, 1 findings, 2 audit unavailable.\nThis is a working-tree check, not a secret detector or Git-history audit."
                );
                return Ok(true);
            }
            _ => return Err("unknown argument; see --help"),
        }
    }
    let root = root.canonicalize().map_err(|_| "repository unavailable")?;
    let identity = Command::new("git")
        .current_dir(&root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|_| "Git unavailable")?;
    if !identity.status.success() {
        return Err("Git repository unavailable");
    }
    let top = std::str::from_utf8(&identity.stdout).map_err(|_| "non-UTF-8 repository path")?;
    if Path::new(top.trim())
        .canonicalize()
        .map_err(|_| "repository unavailable")?
        != root
    {
        return Err("--root must name the Git repository root");
    }
    let mut terms = Vec::new();
    if let Some(path) = terms_path {
        let path = Path::new(&path)
            .canonicalize()
            .map_err(|_| "private terms unavailable")?;
        if path.starts_with(&root) {
            return Err("private terms must remain outside the repository");
        }
        let input = fs::read_to_string(path).map_err(|_| "private terms unreadable")?;
        terms = input
            .trim_start_matches('\u{feff}')
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .collect();
    }
    let output = Command::new("git")
        .current_dir(&root)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()
        .map_err(|_| "Git unavailable")?;
    if !output.status.success() {
        return Err("Git source inventory failed");
    }
    let names = std::str::from_utf8(&output.stdout).map_err(|_| "non-UTF-8 Git paths")?;
    let files: BTreeSet<&str> = names.split('\0').filter(|s| !s.is_empty()).collect();
    let links = Regex::new(r#"!?\[[^\]\r\n]*\]\((<[^>]+>|[^)\r\n]+)\)"#).unwrap();
    let homes = Regex::new(
        r#"(?i)(?:[a-z]:[\\/]+(?:users|home)[\\/]+|/(?:home|Users)/)([a-z0-9][a-z0-9_.-]*)"#,
    )
    .unwrap();
    let mut findings = 0;
    let mut checked = 0;
    let mut report = |name: &str, line: usize, category: &str| {
        findings += 1;
        println!("{}:{line}: {category}", visible(name, &terms));
    };
    for name in files {
        let path = root.join(name);
        if !path.exists() {
            continue; // A tracked deletion is absent from the proposed working tree.
        }
        if !path.is_file() {
            continue;
        }
        if !path
            .canonicalize()
            .map_err(|_| "source path unavailable")?
            .starts_with(&root)
        {
            report(name, 1, "external-source-link");
            continue;
        }
        checked += 1;
        let normalized = name.replace('\\', "/");
        let parts: Vec<_> = normalized.split('/').collect();
        if parts.iter().any(|p| {
            [
                "__pycache__",
                ".pytest_cache",
                ".mypy_cache",
                ".ruff_cache",
                ".codebase-memory",
            ]
            .contains(p)
        }) || ["pyc", "pyo", "jsonl", "log", "sqlite", "sqlite3", "db"]
            .iter()
            .any(|ext| path.extension().is_some_and(|e| e == *ext))
        {
            report(name, 1, "tracked-runtime-artifact");
        }
        if terms.iter().any(|term| name.to_lowercase().contains(term)) {
            report(name, 1, "private-path");
        }
        let bytes = fs::read(&path).map_err(|_| "source file unreadable")?;
        let Ok(source) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let document = path.extension().is_some_and(|ext| ext == "md");
        let shared = normalized == "global/harness.config.toml";
        for (i, line) in source.lines().enumerate() {
            if terms.iter().any(|term| line.to_lowercase().contains(term)) {
                report(name, i + 1, "private-term");
            }
            if (document || shared) && homes.is_match(line) {
                report(name, i + 1, "machine-home-path");
            }
            if shared && line.trim_start().starts_with("[projects.") {
                report(name, i + 1, "shared-project-trust");
            }
        }
        if !document {
            continue;
        }
        for (line_no, line) in prose(source) {
            for matched in links.captures_iter(line) {
                let raw = matched[1].trim();
                let target = if raw.starts_with('<') && raw.ends_with('>') {
                    &raw[1..raw.len() - 1]
                } else {
                    raw.split(" \"").next().unwrap_or(raw)
                };
                if target.contains("://")
                    || target.starts_with("mailto:")
                    || target.contains(['<', '>', '*', '{', '}', '`'])
                {
                    continue;
                }
                let (file, fragment) = target.split_once('#').unwrap_or((target, ""));
                let file = decoded(file);
                let linked = if file.is_empty() {
                    path.clone()
                } else {
                    path.parent().unwrap().join(file)
                };
                let Ok(resolved) = linked.canonicalize() else {
                    report(name, line_no, "missing-local-link");
                    continue;
                };
                if !resolved.starts_with(&root) {
                    report(name, line_no, "external-local-link");
                    continue;
                }
                if !fragment.is_empty()
                    && resolved.extension().is_some_and(|ext| ext == "md")
                    && let Ok(body) = fs::read_to_string(resolved)
                    && !has_anchor(&body, &decoded(fragment))
                {
                    report(name, line_no, "missing-local-anchor");
                }
            }
        }
    }
    println!(
        "Checked {checked} working-tree files; {findings} findings. Git history was not inspected."
    );
    Ok(findings == 0)
}

fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(message) => {
            eprintln!("source audit unavailable: {message}");
            std::process::exit(2);
        }
    }
}
