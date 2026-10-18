//! Model-free working-tree audit. Private audit inputs never belong in the pack.
use harness_core::report_owners::TOKEN_AUDIT_OWNERS;
use harness_core::role_context::Role;
use regex::Regex;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const PRINCIPLES_PATH: &str = "global/principles-of-work.md";
const PRINCIPLES_LIMIT: usize = 24_576;

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

fn kit_invariants(root: &Path, report: &mut impl FnMut(&str, usize, &str)) {
    // Ordinary consumer repositories retain the generic source-audit contract.
    if !root.join("global/kit.json").is_file() {
        return;
    }
    if !root.join(PRINCIPLES_PATH).is_file() {
        report(PRINCIPLES_PATH, 1, "missing-principles");
    }
    for role in [
        harness_core::role_context::Role::Lead,
        harness_core::role_context::Role::Executor,
    ] {
        if harness_core::role_context::resolve(root, role).is_err() {
            // Resolver errors can contain source values or local paths. Keep
            // this publication-boundary audit free of those raw inputs.
            report(
                "global/roles.toml",
                1,
                &format!(
                    "{}-role-context-invalid; repair the descriptor, declared role sources and managed skill memberships",
                    role.label()
                ),
            );
            break;
        }
    }
    for owner in TOKEN_AUDIT_OWNERS {
        let path = Path::new(owner);
        let relative = !path.is_absolute()
            && path
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)));
        let resolved = root.join(path).canonicalize();
        match resolved {
            Ok(target) if relative && target.starts_with(root) && target.is_file() => {}
            Ok(_) => report(
                owner,
                1,
                "invalid-report-owner; keep the owner inside the kit",
            ),
            Err(_) => report(
                owner,
                1,
                "missing-report-owner; restore or update the declared owner",
            ),
        }
    }
}

/// Guidance-load accounting: one deterministic line per measured surface and
/// role, so byte and word totals stay reproducible from the source. Bytes are
/// LF-normalized UTF-8 text bytes, so a Windows CRLF checkout counts the same
/// as the committed LF source; words are whitespace-delimited. The fixed
/// surface counts the
/// common core, the owned developer text, the selected role fragment and the
/// visible catalogue metadata (name plus description) exactly once each. A
/// declared `--invoked role=name` selection adds that role's invoked skill
/// bodies (after frontmatter) and their deduplicated local reference files.
/// Generated assignment briefs stay with the retained `executor_assignment`
/// measurement so no surface is counted twice. These are source-text sizes,
/// not semantic coverage, quality or model behavior.
const ACCOUNTING_UNAVAILABLE: &str =
    "role accounting unavailable: invalid or missing kit role inputs";
const INVOKED_NOT_VISIBLE: &str = "role accounting unavailable: an invoked skill is not visible to that role; check --invoked against roles.toml membership";

#[derive(Default)]
struct LoadCount {
    bytes: usize,
    words: usize,
}

impl LoadCount {
    /// LF-normalized text size, so checkout line endings never change a count.
    fn text(value: &str) -> Self {
        if value.contains('\r') {
            let normalized = value.replace("\r\n", "\n");
            return Self {
                bytes: normalized.len(),
                words: normalized.split_whitespace().count(),
            };
        }
        Self {
            bytes: value.len(),
            words: value.split_whitespace().count(),
        }
    }

    fn add(&mut self, other: Self) {
        self.bytes += other.bytes;
        self.words += other.words;
    }
}

/// Split a skill file into frontmatter and instruction body. The frontmatter
/// is the text between the opening and closing `---` lines; the body follows
/// the closing line. `None` means the file has no frontmatter.
fn skill_frontmatter(source: &str) -> Option<(&str, &str)> {
    let source = source.trim_start_matches('\u{feff}');
    let first = source.split_inclusive('\n').next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let mut offset = first.len();
    for line in source[offset..].split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&source[first.len()..offset], &source[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

fn skill_description(frontmatter: &str) -> String {
    for line in frontmatter.lines() {
        if let Some(value) = line.strip_prefix("description:") {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|inner| inner.strip_suffix('"'))
                .or_else(|| {
                    value
                        .strip_prefix('\'')
                        .and_then(|inner| inner.strip_suffix('\''))
                })
                .unwrap_or(value);
            return value.to_owned();
        }
    }
    String::new()
}

fn accounting_source(root: &Path, relative: &str) -> Result<PathBuf, &'static str> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(ACCOUNTING_UNAVAILABLE);
    }
    Ok(root.join(path))
}

fn accounting_lines(
    root: &Path,
    invoked: &BTreeMap<Role, Vec<String>>,
) -> Result<Vec<String>, &'static str> {
    let manifest_text =
        fs::read_to_string(root.join("global/kit.json")).map_err(|_| ACCOUNTING_UNAVAILABLE)?;
    let manifest: serde_json::Value =
        serde_json::from_str(manifest_text.trim_start_matches('\u{feff}'))
            .map_err(|_| ACCOUNTING_UNAVAILABLE)?;
    if manifest.get("schema").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err(ACCOUNTING_UNAVAILABLE);
    }
    let field = |name: &str| -> Result<String, &'static str> {
        manifest
            .get(name)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or(ACCOUNTING_UNAVAILABLE)
    };
    let instructions = accounting_source(root, &field("instructions")?)?;
    let profile = accounting_source(root, &field("profile")?)?;
    let profile_text = fs::read_to_string(profile).map_err(|_| ACCOUNTING_UNAVAILABLE)?;
    let table: toml::Table = toml::from_str(profile_text.trim_start_matches('\u{feff}'))
        .map_err(|_| ACCOUNTING_UNAVAILABLE)?;
    let developer = match table.get("developer_instructions") {
        None => "",
        Some(toml::Value::String(value)) => value.as_str(),
        Some(_) => return Err(ACCOUNTING_UNAVAILABLE),
    };
    let common =
        LoadCount::text(&fs::read_to_string(instructions).map_err(|_| ACCOUNTING_UNAVAILABLE)?);
    let developer_count = LoadCount::text(developer);
    let links = Regex::new(r#"!?\[[^\]\r\n]*\]\((<[^>]+>|[^)\r\n]+)\)"#)
        .map_err(|_| ACCOUNTING_UNAVAILABLE)?;
    let mut lines = Vec::new();
    for role in [Role::Lead, Role::Executor] {
        let resolved =
            harness_core::role_context::resolve(root, role).map_err(|_| ACCOUNTING_UNAVAILABLE)?;
        let role_text = if developer.is_empty() {
            resolved.developer_instructions.as_str()
        } else {
            resolved
                .developer_instructions
                .strip_prefix(developer)
                .and_then(|rest| rest.strip_prefix("\n\n"))
                .ok_or(ACCOUNTING_UNAVAILABLE)?
        };
        let role_count = LoadCount::text(role_text);
        let selected = invoked.get(&role).map(Vec::as_slice).unwrap_or(&[]);
        let mut catalogue = LoadCount::default();
        let mut visible = 0usize;
        let mut bodies: BTreeMap<&str, (PathBuf, String)> = BTreeMap::new();
        for skill in resolved.skills.iter().filter(|skill| skill.allowed) {
            visible += 1;
            let source = fs::read_to_string(&skill.path).map_err(|_| ACCOUNTING_UNAVAILABLE)?;
            let description = skill_frontmatter(&source)
                .map(|(front, _)| skill_description(front))
                .unwrap_or_default();
            let entry = if description.is_empty() {
                skill.name.clone()
            } else {
                format!("{}: {description}", skill.name)
            };
            catalogue.add(LoadCount::text(&entry));
            bodies.insert(skill.name.as_str(), (skill.path.clone(), source));
        }
        let mut invoked_count = LoadCount::default();
        let mut invoked_skills = 0usize;
        let mut local_references = 0usize;
        let mut local_reference = LoadCount::default();
        let mut external_references = 0usize;
        let mut unresolved_references = 0usize;
        if !selected.is_empty() {
            // Repeated selections and links that point at an invoked body must
            // not count the same text twice, so collect the distinct bodies
            // first and exclude their files from the reference measurement.
            let mut selected_bodies: Vec<(&Path, &str)> = Vec::new();
            let mut invoked_files: BTreeSet<&Path> = BTreeSet::new();
            for name in selected {
                let (skill_file, source) = bodies.get(name.as_str()).ok_or(INVOKED_NOT_VISIBLE)?;
                if invoked_files.insert(skill_file.as_path()) {
                    selected_bodies.push((skill_file.as_path(), source.as_str()));
                }
            }
            invoked_skills = selected_bodies.len();
            let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
            for (skill_file, source) in selected_bodies {
                // `skill_file` names SKILL.md itself, so relative links resolve
                // from its containing directory.
                let directory = skill_file.parent().ok_or(ACCOUNTING_UNAVAILABLE)?;
                let body = skill_frontmatter(source)
                    .map(|(_, body)| body)
                    .unwrap_or(source);
                invoked_count.add(LoadCount::text(body));
                for matched in links.captures_iter(body) {
                    let raw = matched[1].trim();
                    let target = if raw.starts_with('<') && raw.ends_with('>') {
                        &raw[1..raw.len() - 1]
                    } else {
                        raw.split(" \"").next().unwrap_or(raw)
                    };
                    if target.contains("://") || target.starts_with("mailto:") {
                        external_references += 1;
                        continue;
                    }
                    if target.contains(['<', '>', '*', '{', '}', '`']) {
                        continue;
                    }
                    let (file, _) = target.split_once('#').unwrap_or((target, ""));
                    let file = decoded(file);
                    if file.is_empty() {
                        continue;
                    }
                    match directory.join(file.as_str()).canonicalize() {
                        // An invoked body's own file is counted there, not here.
                        Ok(target) if invoked_files.contains(target.as_path()) => {}
                        Ok(target) if target.starts_with(root) && target.is_file() => {
                            if seen.insert(target.clone()) {
                                local_references += 1;
                                match fs::read_to_string(&target) {
                                    Ok(text) => local_reference.add(LoadCount::text(&text)),
                                    Err(_) => {
                                        if let Ok(metadata) = target.metadata() {
                                            local_reference.bytes += metadata.len() as usize;
                                        }
                                    }
                                }
                            }
                        }
                        Ok(_) => external_references += 1,
                        Err(_) => unresolved_references += 1,
                    }
                }
            }
        }
        let fixed = LoadCount {
            bytes: common.bytes + developer_count.bytes + role_count.bytes + catalogue.bytes,
            words: common.words + developer_count.words + role_count.words + catalogue.words,
        };
        lines.push(format!(
            "guidance-accounting role={} surface=fixed visible_skills={visible} metadata_bytes={} metadata_words={} common_bytes={} common_words={} developer_bytes={} developer_words={} role_bytes={} role_words={} fixed_bytes={} fixed_words={}",
            role.label(),
            catalogue.bytes,
            catalogue.words,
            common.bytes,
            common.words,
            developer_count.bytes,
            developer_count.words,
            role_count.bytes,
            role_count.words,
            fixed.bytes,
            fixed.words,
        ));
        if !selected.is_empty() {
            let composed = LoadCount {
                bytes: fixed.bytes + invoked_count.bytes + local_reference.bytes,
                words: fixed.words + invoked_count.words + local_reference.words,
            };
            lines.push(format!(
                "guidance-accounting role={} surface=invoked invoked_skills={} invoked_bytes={} invoked_words={} local_references={local_references} local_reference_bytes={} local_reference_words={} external_references={external_references} unresolved_references={unresolved_references} composed_bytes={} composed_words={} reference_scope=all_links_upper_bound",
                role.label(),
                invoked_skills,
                invoked_count.bytes,
                invoked_count.words,
                local_reference.bytes,
                local_reference.words,
                composed.bytes,
                composed.words,
            ));
        }
    }
    Ok(lines)
}

fn run() -> Result<bool, &'static str> {
    let mut args = env::args().skip(1);
    let mut root = env::current_dir().map_err(|_| "working directory unavailable")?;
    let mut terms_path = None;
    let mut accounting = false;
    let mut invoked: BTreeMap<Role, Vec<String>> = BTreeMap::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = args.next().ok_or("--root requires a directory")?.into(),
            "--private-terms" => {
                terms_path = Some(args.next().ok_or("--private-terms requires a local file")?)
            }
            "--accounting" => accounting = true,
            "--invoked" => {
                let spec = args.next().ok_or("--invoked requires ROLE=NAME[,NAME]")?;
                let (role, names) = spec
                    .split_once('=')
                    .ok_or("--invoked requires ROLE=NAME[,NAME]")?;
                let role = match role {
                    "lead" => Role::Lead,
                    "executor" => Role::Executor,
                    _ => return Err("--invoked role must be lead or executor"),
                };
                let names: Vec<String> = names
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect();
                if names.is_empty() {
                    return Err("--invoked requires at least one skill name");
                }
                invoked.entry(role).or_default().extend(names);
            }
            "--help" | "-h" => {
                println!(
                    "harness-source-check [--root REPOSITORY] [--private-terms LOCAL_FILE]\n  [--accounting [--invoked ROLE=NAME[,NAME]]]\nChecks existing tracked and non-ignored new files, caches, shared trust settings,\nmachine home paths in documentation/configuration, caller-supplied private terms,\nlocal inline Markdown links and the 24,576-byte portable-principles limit. Kit roots\nalso require valid role sources and skill memberships, principles and token-audit\ndocumentation owners.\nSkips fenced examples, template targets and non-text assets. Terms are one per\nline in a file outside the repository. Diagnostics omit matched text.\n--accounting also prints one reproducible guidance-load line per role for the\nfixed load (visible catalogue metadata, common core, developer text and the\nselected role fragment) and, for each declared --invoked ROLE=NAME[,NAME]\nselection, that role's distinct invoked skill bodies after frontmatter plus\nevery distinct local link target as a reference upper bound: repeated names\ncount once, a link to another invoked body counts once under invoked bodies,\nand links inside linked files are not followed. Required and optional links are\nnot distinguished; every invoked line carries reference_scope=all_links_upper_bound.\nLF-normalized bytes and whitespace words stay separate;\ngenerated briefs are measured once by the retained assignment-renderer tests.\n--accounting needs valid kit role inputs and exits 2 when its inputs are\ninvalid, after any audit findings are printed.\nExit: 0 clean, 1 findings, 2 audit unavailable.\nThis is a working-tree check, not a secret detector or Git-history audit."
                );
                return Ok(true);
            }
            _ => return Err("unknown argument; see --help"),
        }
    }
    if !invoked.is_empty() && !accounting {
        return Err("--invoked requires --accounting");
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
    kit_invariants(&root, &mut report);
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
                ".codegraph-harness-store",
                ".codegraph-harness-active",
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
        if normalized == PRINCIPLES_PATH && bytes.len() > PRINCIPLES_LIMIT {
            report(
                name,
                1,
                &format!(
                    "principles-size-limit actual={} allowed={PRINCIPLES_LIMIT}; shorten without weakening requirements",
                    bytes.len()
                ),
            );
        }
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
    if accounting {
        for line in accounting_lines(&root, &invoked)? {
            println!("{line}");
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
