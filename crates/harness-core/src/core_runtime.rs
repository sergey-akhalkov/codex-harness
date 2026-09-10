//! Model-free core runtime validation for an installed native Codex CLI.
//! The parent owns wiring, Cargo and lifecycle orchestration.
#![cfg(windows)]

use crate::process::{CommandSpec, StopReason};
use crate::{build_identity, native_build};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    cmp::Ordering,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
const INSTRUCTIONS_LIMIT: usize = 1024 * 1024;
const MINIMUM: &str = "0.153.4";

/// Compact public receipt. Raw stdout/stderr stay in `evidence`.
#[derive(Debug, Serialize)]
pub struct RuntimeReceipt {
    pub schema: u32,
    pub passed: bool,
    pub model_calls: u32,
    pub executable: PathBuf,
    pub executable_sha256: String,
    pub evidence: PathBuf,
}

/// Uses the caller-selected original native Codex executable for `--version` and
/// `--help`. The installed launcher is the `CODEX_HOME/harness/bin/codex.exe`
/// symlink to the immutable native build and is executed for `debug prompt-input`.
/// No PATH lookup, shell host, authentication copy, live home write or model
/// request is involved. Private stdout/stderr remain in `evidence`.
pub fn verify(
    upstream: &Path,
    launcher: &Path,
    codex_home: &Path,
    instructions: &[u8],
    timeout: Duration,
) -> io::Result<RuntimeReceipt> {
    if !absolute_exe(upstream)
        || !launcher.is_absolute()
        || !codex_home.is_absolute()
        || timeout.is_zero()
        || Instant::now().checked_add(timeout).is_none()
    {
        return Err(io::Error::other(
            "Absolute native executable, launcher, home and finite timeout are required.",
        ));
    }
    if instructions.is_empty()
        || instructions.len() > INSTRUCTIONS_LIMIT
        || std::str::from_utf8(instructions).is_err()
    {
        return Err(io::Error::other(
            "Invalid native instruction size or encoding.",
        ));
    }
    crate::installation_state::normal(upstream)?;
    crate::installation_state::normal(launcher)?;
    crate::installation_state::normal(codex_home)?;
    crate::inventory::ordinary_parents(&codex_home.join("harness"))?;
    crate::inventory::ordinary_parents(launcher)?;
    build_identity::ordinary(upstream)?;
    if !fs::metadata(codex_home)?.is_dir() {
        return Err(io::Error::other(
            "Native runtime home must be an existing ordinary directory.",
        ));
    }
    let expected_launcher = codex_home.join("harness").join("bin").join("codex.exe");
    if crate::installation_state::normal(launcher)?
        != crate::installation_state::normal(&expected_launcher)?
    {
        return Err(io::Error::other(
            "Installed launcher is not the expected harness command.",
        ));
    }
    let workdir = codex_home.join("harness");
    if !fs::metadata(&workdir)?.is_dir() {
        return Err(io::Error::other(
            "Native runtime working directory is missing.",
        ));
    }
    let native_launcher = resolve_native_launcher(launcher)?;
    let until = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::other("Native runtime deadline overflow."))?;
    let executable_sha256 = build_identity::hash_file(upstream)?;
    if native_launcher.canonicalize()? == upstream.canonicalize()?
        || build_identity::hash_file(&native_launcher)? == executable_sha256
    {
        return Err(io::Error::other(
            "Installed launcher points to the original CLI; explicit repair is required.",
        ));
    }
    let root = tempfile::Builder::new()
        .prefix("harness-native-runtime-")
        .tempdir()?
        .keep();
    let work = root.join("work");
    fs::create_dir(&work)?;
    let result = (|| {
        let version = invoke(
            upstream,
            &["--version"],
            &work,
            Some(&work),
            &root,
            "version",
            remaining(until)?,
        )?;
        let version_number = supported_cli(utf8(&bounded(
            &root.join("version.stdout.txt"),
            OUTPUT_LIMIT,
        )?)?)?;

        let help = invoke(
            upstream,
            &["--help"],
            &work,
            Some(&work),
            &root,
            "help",
            remaining(until)?,
        )?;
        if !has_file_profile(utf8(&bounded(
            &root.join("help.stdout.txt"),
            OUTPUT_LIMIT,
        )?)?) {
            return Err(io::Error::other(
                "This Codex CLI does not expose the required file-profile contract.",
            ));
        }

        let prompt = invoke(
            launcher,
            &["debug", "prompt-input"],
            &workdir,
            Some(codex_home),
            &root,
            "prompt",
            remaining(until)?,
        )?;
        inspect_prompt(
            &bounded(&root.join("prompt.stdout.txt"), OUTPUT_LIMIT)?,
            &normalize_instructions(instructions),
        )?;
        if build_identity::hash_file(upstream)? != executable_sha256 {
            return Err(io::Error::other(
                "Native executable changed during runtime validation.",
            ));
        }
        if resolve_native_launcher(launcher)? != native_launcher {
            return Err(io::Error::other(
                "Installed launcher changed during runtime validation.",
            ));
        }
        let receipt = RuntimeReceipt {
            schema: 1,
            passed: true,
            model_calls: 0,
            executable: upstream.to_owned(),
            executable_sha256: executable_sha256.clone(),
            evidence: root.clone(),
        };
        fs::write(
            root.join("receipt.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "passed": true,
                "model_calls": 0,
                "executable": upstream,
                "executable_sha256": executable_sha256,
                "launcher": launcher,
                "native_launcher": native_launcher,
                "codex_home": codex_home,
                "working_directory": workdir,
                "full_global_instructions": true,
                "approval_policy": "never",
                "sandbox": "danger-full-access",
                "cli_version": version_number,
                "version_exit": version.exit_code,
                "help_exit": help.exit_code,
                "prompt_exit": prompt.exit_code
            }))?,
        )?;
        Ok(receipt)
    })();
    result.map_err(|error: io::Error| {
        let _ = fs::write(
            root.join("failure.json"),
            serde_json::to_vec(
                &json!({"error":error.to_string(), "raw_os_error":error.raw_os_error()}),
            )
            .unwrap_or_default(),
        );
        io::Error::new(
            error.kind(),
            format!(
                "Native runtime validation failed. Private evidence: {}",
                root.display()
            ),
        )
    })
}

fn remaining(until: Instant) -> io::Result<Duration> {
    let remaining = until.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(io::Error::other("Native runtime validation timed out."));
    }
    Ok(remaining)
}

fn resolve_native_launcher(launcher: &Path) -> io::Result<PathBuf> {
    let meta = fs::symlink_metadata(launcher)?;
    if !meta.file_type().is_symlink() {
        return Err(io::Error::other(
            "Installed launcher must be a native command symlink.",
        ));
    }
    let target = fs::read_link(launcher)?;
    if !absolute_exe(&target) || target.file_name() != Some(std::ffi::OsStr::new("codex.exe")) {
        return Err(io::Error::other(
            "Installed launcher does not target the native build command.",
        ));
    }
    crate::inventory::ordinary_parents(&target)?;
    build_identity::ordinary(&target)?;
    if target.canonicalize()? != launcher.canonicalize()? {
        return Err(io::Error::other(
            "Installed launcher does not resolve to its intended native destination.",
        ));
    }
    target.canonicalize()
}

fn invoke(
    program: &Path,
    args: &[&str],
    current_dir: &Path,
    home: Option<&Path>,
    root: &Path,
    step: &str,
    timeout: Duration,
) -> io::Result<crate::process::Outcome> {
    let mut command = CommandSpec::new(program);
    command.args = args.iter().map(|value| (*value).into()).collect();
    command.current_dir = Some(current_dir.to_owned());
    if let Some(home) = home {
        command
            .env
            .insert("CODEX_HOME".into(), Some(home.as_os_str().to_owned()));
    }
    let outcome = native_build::invoke_management(
        command,
        &root.join(format!("{step}.stderr.txt")),
        Some(&root.join(format!("{step}.stdout.txt"))),
        timeout,
    )?;
    fs::write(
        root.join(format!("{step}.process.json")),
        serde_json::to_vec_pretty(&outcome)?,
    )?;
    if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
        return Err(io::Error::other(format!(
            "Native runtime {step} command did not succeed."
        )));
    }
    Ok(outcome)
}

fn bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::other("Native runtime output exceeds its bound."));
    }
    Ok(bytes)
}

fn utf8(bytes: &[u8]) -> io::Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| io::Error::other("Native runtime output is not UTF-8."))
}

fn supported_cli(text: &str) -> io::Result<String> {
    let version = parse_cli_version(text)?;
    if compare_version(&version, MINIMUM) == Ordering::Less {
        return Err(io::Error::other(
            "Installed Codex CLI is below the supported file-profile contract.",
        ));
    }
    Ok(version)
}

fn parse_cli_version(text: &str) -> io::Result<String> {
    let text = text.trim();
    let Some(rest) = text.strip_prefix("codex-cli ") else {
        return Err(io::Error::other("Cannot verify Codex CLI version."));
    };
    let version: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if parse_parts(&version).is_none() {
        return Err(io::Error::other("Cannot verify Codex CLI version."));
    }
    Ok(version)
}

fn parse_parts(version: &str) -> Option<[u64; 3]> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some([major, minor, patch])
}

fn compare_version(observed: &str, minimum: &str) -> Ordering {
    parse_parts(observed)
        .unwrap_or([0, 0, 0])
        .cmp(&parse_parts(minimum).unwrap_or([0, 0, 0]))
}

fn has_file_profile(help: &str) -> bool {
    help.contains("<name>.config.toml") && help.contains("--profile")
}

fn normalize_instructions(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .replace("\r\n", "\n")
        .trim()
        .to_owned()
}

fn inspect_prompt(payload: &[u8], expected: &str) -> io::Result<()> {
    let value: Value = serde_json::from_slice(payload)
        .map_err(|_| io::Error::other("Native prompt-input output is not valid JSON."))?;
    let texts = prompt_texts(&value);
    let combined = texts.join("\n").replace("\r\n", "\n");
    if expected.is_empty() || !combined.contains(expected) {
        return Err(io::Error::other(
            "The complete global instruction source was not loaded.",
        ));
    }
    let permissions = texts
        .iter()
        .filter(|text| text.contains("Filesystem sandboxing defines"))
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    if !permissions.contains("danger-full-access")
        || !permissions.contains("Approval policy is currently never")
    {
        return Err(io::Error::other(
            "Shared Full Access defaults did not resolve in the neutral startup check.",
        ));
    }
    Ok(())
}

fn prompt_texts(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items.iter().flat_map(prompt_texts).collect(),
        Value::Object(map) => {
            let mut texts = Vec::new();
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                texts.push(text.to_owned());
            }
            for nested in map.values() {
                if !nested.is_string() {
                    texts.extend(prompt_texts(nested));
                }
            }
            texts
        }
        _ => Vec::new(),
    }
}

fn absolute_exe(path: &Path) -> bool {
    path.is_absolute()
        && path
            .extension()
            .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_help_and_prompt_interpretation_match_supported_cli_contract() {
        assert_eq!(parse_cli_version("codex-cli 0.153.4\n").unwrap(), "0.153.4");
        assert_eq!(
            parse_cli_version("codex-cli 0.160.0 (native)\n").unwrap(),
            "0.160.0"
        );
        assert!(supported_cli("codex-cli 0.153.4\n").is_ok());
        assert!(supported_cli("codex-cli 0.153.3\n").is_err());
        assert!(parse_cli_version("codex 0.153.4").is_err());
        assert!(parse_cli_version("codex-cli not-a-version").is_err());
        assert_eq!(compare_version("0.153.4", MINIMUM), Ordering::Equal);
        assert_eq!(compare_version("0.153.3", MINIMUM), Ordering::Less);
        assert_eq!(compare_version("0.154.0", MINIMUM), Ordering::Greater);

        let help = "Codex CLI\n\nUsage: codex [OPTIONS] [PROMPT]\n      --profile <PROFILE>          Configuration profile from config.toml\n      Select <name>.config.toml with --profile.\n";
        assert!(has_file_profile(help));
        assert!(!has_file_profile("Usage: codex --permission-profile"));
        assert!(!has_file_profile("Usage: codex --profile other"));

        let instructions = "Use capability levels to finish verified work.\nKeep reusable artifacts project-neutral.";
        let payload = json!([
            {
                "type": "message",
                "role": "developer",
                "content": [
                    {"type": "input_text", "text": instructions},
                    {
                        "type": "input_text",
                        "text": "<permissions instructions>\nFilesystem sandboxing defines which files can be read or written. `sandbox_mode` is `danger-full-access`: No filesystem sandboxing - all commands are permitted.\nApproval policy is currently never.\n</permissions instructions>"
                    }
                ]
            }
        ]);
        inspect_prompt(payload.to_string().as_bytes(), instructions).unwrap();
        let wrapped = json!({
            "entry": r"C:\\owned\\harness\\bin\\codex.exe",
            "prompt": payload
        });
        inspect_prompt(wrapped.to_string().as_bytes(), instructions).unwrap();
        let missing_instructions = json!([{
            "content": [{
                "text": "<permissions instructions>\nFilesystem sandboxing defines danger-full-access\nApproval policy is currently never\n"
            }]
        }]);
        assert_eq!(
            inspect_prompt(missing_instructions.to_string().as_bytes(), instructions)
                .unwrap_err()
                .to_string(),
            "The complete global instruction source was not loaded."
        );
        let missing_permissions = json!([{ "content": [{ "text": instructions }] }]);
        assert_eq!(
            inspect_prompt(missing_permissions.to_string().as_bytes(), instructions)
                .unwrap_err()
                .to_string(),
            "Shared Full Access defaults did not resolve in the neutral startup check."
        );
        let malformed = b"AUTH_TOKEN=PRIVATE_RUNTIME_SENTINEL {not-json";
        let error = inspect_prompt(malformed, instructions).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Native prompt-input output is not valid JSON."
        );
        assert!(!error.to_string().contains("PRIVATE_RUNTIME_SENTINEL"));
    }

    #[test]
    fn invalid_inputs_never_launch_or_echo_instructions() {
        let secret = b"PRIVATE_RUNTIME_SENTINEL";
        for timeout in [Duration::ZERO, Duration::MAX] {
            let error = verify(
                Path::new("C:/owned/codex.exe"),
                Path::new("C:/owned/harness/bin/codex.exe"),
                Path::new("C:/owned"),
                secret,
                timeout,
            )
            .unwrap_err();
            assert!(error.to_string().contains("finite timeout"));
            assert!(!error.to_string().contains("PRIVATE_RUNTIME_SENTINEL"));
        }
        for (upstream, launcher, home) in [
            (
                Path::new("codex.exe"),
                Path::new("C:/owned/harness/bin/codex.exe"),
                Path::new("C:/owned"),
            ),
            (
                Path::new("C:/abs/codex.ps1"),
                Path::new("C:/owned/harness/bin/codex.exe"),
                Path::new("C:/owned"),
            ),
            (
                Path::new("C:/abs/codex.exe"),
                Path::new("C:/other/harness/bin/codex.exe"),
                Path::new("C:/owned"),
            ),
        ] {
            let error =
                verify(upstream, launcher, home, secret, Duration::from_secs(1)).unwrap_err();
            assert!(!error.to_string().contains("PRIVATE_RUNTIME_SENTINEL"));
        }
        assert!(
            verify(
                Path::new("C:/abs/codex.exe"),
                Path::new("C:/abs/harness/bin/codex.exe"),
                Path::new("C:/abs"),
                &[255],
                Duration::from_secs(1)
            )
            .is_err()
        );
        assert!(
            verify(
                Path::new("C:/abs/codex.exe"),
                Path::new("C:/abs/harness/bin/codex.exe"),
                Path::new("C:/abs"),
                &vec![b' '; INSTRUCTIONS_LIMIT + 1],
                Duration::from_secs(1)
            )
            .is_err()
        );
        assert!(
            verify(
                Path::new("C:/abs/codex.exe"),
                Path::new("C:/abs/harness/bin/codex.exe"),
                Path::new("C:/abs"),
                b"",
                Duration::from_secs(1)
            )
            .is_err()
        );
    }

    #[test]
    fn failed_command_errors_keep_exact_private_output_out_of_public_text() {
        let root = tempfile::Builder::new()
            .prefix("core-runtime-failure-")
            .tempdir()
            .unwrap()
            .keep();
        // The Rust test runner itself rejects this argument before running any
        // tests. Exercise the real bounded subprocess failure and private logs.
        let error = invoke(
            &std::env::current_exe().unwrap(),
            &["--PRIVATE_RUNTIME_SENTINEL"],
            &root,
            Some(&root),
            &root,
            "prompt",
            Duration::from_secs(10),
        )
        .unwrap_err();
        assert!(error.to_string().contains("prompt command did not succeed"));
        assert!(!error.to_string().contains("PRIVATE_RUNTIME_SENTINEL"));
        assert!(
            fs::read_to_string(root.join("prompt.stderr.txt"))
                .unwrap()
                .contains("PRIVATE_RUNTIME_SENTINEL")
        );
        let record: Value =
            serde_json::from_slice(&fs::read(root.join("prompt.process.json")).unwrap()).unwrap();
        assert!(record["exit_code"].as_u64().is_some_and(|code| code != 0));
    }
}
