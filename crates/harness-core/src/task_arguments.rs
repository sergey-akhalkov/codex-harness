//! Session argument planning for harness-managed interactive Codex sessions.
//!
//! [plan](fn.plan.html) classifies a native "codex" argv (excluding the
//! program name) and decides whether it starts an ordinary interactive
//! session that the harness can serve by pairing its own "codex app-server"
//! process with the native TUI in "--remote" mode.
//!
//! * ManagedArguments::tui keeps the caller's arguments verbatim. The caller
//!   owns adding its "--remote <ADDR>" endpoint, which must be inserted
//!   before any "--" separator.
//! * ManagedArguments::backend lists the arguments to pass after the
//!   "app-server" subcommand. User "-c/--config", "--enable", "--disable"
//!   and "--strict-config" entries keep their encounter order, followed by
//!   mapped "-c" overrides for session flags ("-m", "-s", "-a",
//!   "--approve-for-me", "--dangerously-bypass-approvals-and-sandbox",
//!   "--add-dir") that the app-server only accepts as config overrides.
//!   Appending the mapped entries mirrors the official configuration
//!   contract, which places CLI flags and "-c" overrides in the same top
//!   precedence layer (learn.chatgpt.com/docs/config-file/config-basic,
//!   section "Configuration precedence"), so explicit flag identities keep
//!   winning over earlier generic overrides.
//! * ManagedArguments::cwd applies the last "-C/--cd" value to "cwd". The
//!   TUI arguments still contain "-C", so the TUI process itself must start
//!   from the original "cwd"; starting it from the resolved cwd would apply
//!   a relative "-C" twice.
//!
//! "Ok(None)" keeps the existing native bypass: explicit "--remote" or
//! "--profile/-p", help/version, and every noninteractive subcommand.
//! "Err" asks the caller to fall back to an ordinary native launch:
//! "ErrorKind::InvalidInput" mirrors argv shapes the native CLI itself
//! rejects, and "ErrorKind::Unsupported" names recognized flags whose
//! backend effect cannot be reproduced faithfully, so they are reported
//! instead of silently discarded. "--remote-auth-token-env" stays in the
//! TUI arguments verbatim; the caller owns making its owned endpoint accept
//! that token.
//!
//! The recognized surface is bounded to the inspected "codex-cli" 0.154.0
//! help and argv probes: "--full-auto" does not exist there and "-a"
//! accepts only "on-request" and "never".

use std::ffi::OsString;
use std::io::{self, ErrorKind};
use std::mem::take;
use std::path::{Path, PathBuf};

/// Arguments for one harness-managed interactive session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedArguments {
    /// Arguments to pass after the "app-server" subcommand: the user's
    /// "-c/--config", "--enable", "--disable" and "--strict-config" entries
    /// in encounter order, followed by the mapped overrides for the native
    /// session flags.
    pub backend: Vec<OsString>,
    /// The caller's arguments, verbatim, for the native remote TUI. Prompt
    /// words, "--" separators, image values and TUI-only flags
    /// ("--worktree", "--no-alt-screen", "--last", ...) stay exactly as
    /// given; "-C/--cd" also stays, so the TUI must start from the original
    /// "cwd" (see ManagedArguments::cwd).
    pub tui: Vec<OsString>,
    /// TUI arguments for attaching to a newly controller-created thread.
    /// Permission options are already applied by `backend`; remote resume
    /// rejects those options even when their values match the saved thread.
    pub attachment: Vec<OsString>,
    /// "cwd" with the last "-C/--cd" value applied; relative values resolve
    /// against "cwd". Nothing is canonicalized and no filesystem access
    /// happens, so invalid directories still reach the native CLI unchanged.
    pub cwd: PathBuf,
    /// Ordinary new interaction; resume/fork keep their native selection flow.
    pub new_session: bool,
}

/// First positional tokens that dispatch noninteractive native subcommands.
/// Unknown first tokens are ordinary prompts, exactly like the native CLI's
/// "[PROMPT]" positional.
const NONSESSION_COMMANDS: &[&str] = &[
    "agents",
    "exec",
    "e",
    "review",
    "login",
    "logout",
    "mcp",
    "plugin",
    "mcp-server",
    "app-server",
    "remote-control",
    "app",
    "completion",
    "update",
    "doctor",
    "sandbox",
    "debug",
    "apply",
    "a",
    "queue",
    "archive",
    "delete",
    "migrate-rollouts",
    "unarchive",
    "cloud",
    "exec-server",
    "features",
    "help",
];

const SANDBOX_MODES: &[&str] = &["read-only", "workspace-write", "danger-full-access"];
const APPROVAL_POLICIES: &[&str] = &["on-request", "never"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionKind {
    /// "codex [OPTIONS] [PROMPT]": at most one positional prompt.
    Prompt,
    /// "codex resume [OPTIONS] [SESSION_ID] [PROMPT]".
    Resume,
    /// "codex fork [OPTIONS] [SESSION_ID] [PROMPT]".
    Fork,
}

#[derive(Default)]
struct Session {
    backend_user: Vec<OsString>,
    config_values: Vec<String>,
    model: Option<String>,
    sandbox: Option<String>,
    approval: Option<String>,
    approve_for_me: bool,
    bypass_sandbox: bool,
    add_dirs: Vec<String>,
    cd: Option<OsString>,
}

struct Parser<'a> {
    args: &'a [OsString],
    attachment: Vec<OsString>,
    session: Session,
    kind: Option<SessionKind>,
    positionals: usize,
    separator: bool,
}

/// Classifies a native "codex" argv (without the program name) for a
/// harness-managed interactive session.
///
/// The input must already be native-shaped: harness-internal options such
/// as "--harness-effort" have to be rewritten away before planning.
/// Ordinary sessions, including "resume" and "fork", plan a managed
/// app-server plus remote TUI pair; everything else keeps its existing
/// native behavior as described in the module documentation.
pub fn plan(args: &[OsString], cwd: &Path) -> io::Result<Option<ManagedArguments>> {
    let mut parser = Parser {
        args,
        attachment: Vec::new(),
        session: Session::default(),
        kind: None,
        positionals: 0,
        separator: false,
    };
    if !parser.run()? {
        return Ok(None);
    }
    let session = take(&mut parser.session);
    let (backend, directory) = finalize(&session, cwd)?;
    if parser.kind == Some(SessionKind::Resume) && parser.attachment.len() != args.len() {
        return Err(unsupported(
            "native remote resume rejects permission overrides; preserve the ordinary native resume path",
        ));
    }
    Ok(Some(ManagedArguments {
        backend,
        tui: args.to_vec(),
        attachment: parser.attachment,
        cwd: directory,
        new_session: !matches!(parser.kind, Some(SessionKind::Resume | SessionKind::Fork)),
    }))
}

impl Parser<'_> {
    /// Returns false when the invocation keeps its existing native bypass.
    fn run(&mut self) -> io::Result<bool> {
        let args = self.args;
        let mut i = 0usize;
        while let Some(arg) = args.get(i) {
            let start = i;
            i += 1;
            let Some(text) = arg.to_str() else {
                return Err(invalid(
                    "the native CLI rejects this argv: arguments must be valid UTF-8",
                ));
            };
            if self.separator {
                if !self.record_positional(text, false)? {
                    return Ok(false);
                }
                self.attachment.push(arg.clone());
                continue;
            }
            if text == "--" {
                self.separator = true;
                self.attachment.push(arg.clone());
                continue;
            }
            if let Some(long) = text.strip_prefix("--") {
                if !self.long_flag(long, args, &mut i)? {
                    return Ok(false);
                }
                self.record_attachment_options(&args[start..i]);
                continue;
            }
            if text.len() > 1 && text.starts_with('-') {
                if !self.short_flags(text, args, &mut i)? {
                    return Ok(false);
                }
                self.record_attachment_options(&args[start..i]);
                continue;
            }
            if !self.record_positional(text, true)? {
                return Ok(false);
            }
            self.attachment.push(arg.clone());
        }
        Ok(true)
    }

    fn record_attachment_options(&mut self, options: &[OsString]) {
        // This is one already-parsed option and its values, never a prompt or
        // an image value that merely looks like a flag.
        let flag = options[0].to_str().unwrap();
        let name = flag.split('=').next().unwrap();
        if matches!(
            name,
            "--sandbox"
                | "--ask-for-approval"
                | "--approve-for-me"
                | "--dangerously-bypass-approvals-and-sandbox"
                | "--add-dir"
        ) || (!flag.starts_with("--") && (flag.starts_with("-s") || flag.starts_with("-a")))
        {
            return;
        }
        let config =
            if flag == "--config" || flag.starts_with("--config=") || flag.starts_with("-c") {
                self.session.config_values.last().map(String::as_str)
            } else {
                None
            };
        if let Some((key, _)) = config.and_then(|value| value.split_once('=')) {
            let key = key.trim();
            if matches!(
                key,
                "approval_policy"
                    | "approvals_reviewer"
                    | "sandbox_mode"
                    | "sandbox_workspace_write"
            ) || key.starts_with("sandbox_workspace_write.")
            {
                return;
            }
        }
        self.attachment.extend_from_slice(options);
    }

    fn record_positional(&mut self, text: &str, allow_command: bool) -> io::Result<bool> {
        if self.kind.is_none() {
            if allow_command && matches!(text, "resume" | "fork") {
                self.kind = Some(if text == "resume" {
                    SessionKind::Resume
                } else {
                    SessionKind::Fork
                });
                return Ok(true);
            }
            if allow_command && NONSESSION_COMMANDS.contains(&text) {
                return Ok(false);
            }
            self.kind = Some(SessionKind::Prompt);
        }
        self.positionals += 1;
        let limit = match self.kind {
            Some(SessionKind::Prompt) => 1,
            Some(SessionKind::Resume | SessionKind::Fork) => 2,
            None => unreachable!("a session kind is always set above"),
        };
        if self.positionals > limit {
            return Err(invalid(format!(
                "the native CLI rejects this argv: unexpected argument '{text}'"
            )));
        }
        Ok(true)
    }

    fn long_flag(&mut self, long: &str, args: &[OsString], i: &mut usize) -> io::Result<bool> {
        let (name, attached) = match long.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (long, None),
        };
        match name {
            "help" | "version" => {
                reject_attached(name, attached)?;
                Ok(false)
            }
            "remote" | "profile" => {
                let value = match attached {
                    Some(value) => OsString::from(value),
                    None => self.take_value(args, i, &format!("--{name}"))?,
                };
                if value.to_str().is_none() {
                    return Err(invalid(format!(
                        "the native CLI rejects this argv: --{name} requires a UTF-8 value"
                    )));
                }
                Ok(false)
            }
            "config" => {
                let value = match attached {
                    Some(value) => OsString::from(value),
                    None => self.take_value(args, i, "--config")?,
                };
                let Some(text) = value.to_str() else {
                    return Err(invalid(
                        "the native CLI rejects this argv: --config requires a UTF-8 value",
                    ));
                };
                validate_override(text)?;
                self.session.config_values.push(text.to_owned());
                self.session.backend_user.extend(["-c".into(), value]);
                Ok(true)
            }
            "enable" | "disable" => {
                let value = match attached {
                    Some(value) => OsString::from(value),
                    None => self.take_value(args, i, &format!("--{name}"))?,
                };
                if value.to_str().is_none() {
                    return Err(invalid(format!(
                        "the native CLI rejects this argv: --{name} requires a UTF-8 value"
                    )));
                }
                self.session
                    .backend_user
                    .extend([OsString::from(format!("--{name}")), value]);
                Ok(true)
            }
            "strict-config" => {
                reject_attached(name, attached)?;
                self.session.backend_user.push("--strict-config".into());
                Ok(true)
            }
            "model" => {
                let value = self.utf8_value(attached, args, i, "--model")?;
                self.session.model = Some(value);
                Ok(true)
            }
            "sandbox" => {
                let value = self.utf8_value(attached, args, i, "--sandbox")?;
                self.session.sandbox = Some(validate_enum(&value, "--sandbox", SANDBOX_MODES)?);
                Ok(true)
            }
            "ask-for-approval" => {
                let value = self.utf8_value(attached, args, i, "--ask-for-approval")?;
                self.session.approval = Some(validate_enum(
                    &value,
                    "--ask-for-approval",
                    APPROVAL_POLICIES,
                )?);
                Ok(true)
            }
            "approve-for-me" => {
                reject_attached(name, attached)?;
                self.session.approve_for_me = true;
                Ok(true)
            }
            "dangerously-bypass-approvals-and-sandbox" => {
                reject_attached(name, attached)?;
                self.session.bypass_sandbox = true;
                Ok(true)
            }
            "cd" => {
                let value = match attached {
                    Some(value) => OsString::from(value),
                    None => self.take_value(args, i, "--cd")?,
                };
                self.session.cd = Some(value);
                Ok(true)
            }
            "add-dir" => {
                let value = self.utf8_value(attached, args, i, "--add-dir")?;
                self.session.add_dirs.push(value);
                Ok(true)
            }
            "image" => match attached {
                Some(_) => Ok(true),
                None => self.collect_images(args, i),
            },
            "worktree" | "no-alt-screen" => {
                reject_attached(name, attached)?;
                Ok(true)
            }
            "remote-auth-token-env" => {
                let _ = self.utf8_value(attached, args, i, "--remote-auth-token-env")?;
                Ok(true)
            }
            "local-provider" => {
                let value = self.utf8_value(attached, args, i, "--local-provider")?;
                Err(unsupported(format!(
                    "--local-provider '{value}' selects an open-source provider; no verified app-server config mapping exists for codex-cli 0.154.0"
                )))
            }
            "oss" => {
                reject_attached(name, attached)?;
                Err(unsupported(
                    "--oss selects the open-source provider; no verified app-server config mapping exists for codex-cli 0.154.0",
                ))
            }
            "search" => {
                reject_attached(name, attached)?;
                Err(unsupported(
                    "--search enables live web search; its config mapping for the app-server is not verified for codex-cli 0.154.0",
                ))
            }
            "dangerously-bypass-hook-trust" => {
                reject_attached(name, attached)?;
                Err(unsupported(
                    "--dangerously-bypass-hook-trust changes hook-trust handling for this invocation; its effect on a harness-owned app-server is not established",
                ))
            }
            "last" | "all"
                if matches!(self.kind, Some(SessionKind::Resume | SessionKind::Fork)) =>
            {
                reject_attached(name, attached)?;
                Ok(true)
            }
            "include-non-interactive" if self.kind == Some(SessionKind::Resume) => {
                reject_attached(name, attached)?;
                Ok(true)
            }
            _ => Err(invalid(format!(
                "the native CLI rejects this argv: unknown option '--{name}'"
            ))),
        }
    }

    fn short_flags(&mut self, text: &str, args: &[OsString], i: &mut usize) -> io::Result<bool> {
        let cluster = &text[1..];
        let mut chars = cluster.char_indices();
        if let Some((index, flag)) = chars.next() {
            let rest = &cluster[index + flag.len_utf8()..];
            match flag {
                'h' | 'V' => return Ok(false),
                'p' => {
                    let _ = self.attached_or_next_value(rest, args, i, "-p")?;
                    return Ok(false);
                }
                'c' => {
                    let value = self.attached_or_next_value(rest, args, i, "-c")?;
                    let Some(value) = value.to_str() else {
                        return Err(invalid(
                            "the native CLI rejects this argv: -c requires a UTF-8 value",
                        ));
                    };
                    validate_override(value)?;
                    self.session
                        .backend_user
                        .extend(["-c".into(), value.into()]);
                    self.session.config_values.push(value.to_owned());
                    return Ok(true);
                }
                'C' => {
                    let value = self.attached_or_next_value(rest, args, i, "-C")?;
                    self.session.cd = Some(value);
                    return Ok(true);
                }
                'm' => {
                    let value = self.attached_or_next_value(rest, args, i, "-m")?;
                    self.session.model = Some(utf8_owned(value, "-m")?);
                    return Ok(true);
                }
                's' => {
                    let value = self.attached_or_next_value(rest, args, i, "-s")?;
                    let value = utf8_owned(value, "-s")?;
                    self.session.sandbox = Some(validate_enum(&value, "--sandbox", SANDBOX_MODES)?);
                    return Ok(true);
                }
                'a' => {
                    let value = self.attached_or_next_value(rest, args, i, "-a")?;
                    let value = utf8_owned(value, "-a")?;
                    self.session.approval = Some(validate_enum(
                        &value,
                        "--ask-for-approval",
                        APPROVAL_POLICIES,
                    )?);
                    return Ok(true);
                }
                'i' => {
                    if rest.is_empty() {
                        self.collect_images(args, i)?;
                    }
                    return Ok(true);
                }
                other => {
                    return Err(invalid(format!(
                        "the native CLI rejects this argv: unknown option '-{other}'"
                    )));
                }
            }
        }
        Ok(true)
    }

    fn collect_images(&mut self, args: &[OsString], i: &mut usize) -> io::Result<bool> {
        let mut taken = false;
        while let Some(next) = args.get(*i) {
            let bytes = next.as_os_str().as_encoded_bytes();
            if bytes.first() == Some(&b'-') {
                break;
            }
            *i += 1;
            taken = true;
        }
        if !taken {
            return Err(invalid(
                "the native CLI rejects this argv: --image requires at least one value",
            ));
        }
        Ok(true)
    }

    fn utf8_value(
        &self,
        attached: Option<&str>,
        args: &[OsString],
        i: &mut usize,
        flag: &str,
    ) -> io::Result<String> {
        match attached {
            Some(value) => Ok(value.to_owned()),
            None => {
                let value = self.take_value(args, i, flag)?;
                utf8_owned(value, flag)
            }
        }
    }

    fn attached_or_next_value(
        &self,
        rest: &str,
        args: &[OsString],
        i: &mut usize,
        flag: &str,
    ) -> io::Result<OsString> {
        if rest.is_empty() {
            return self.take_value(args, i, flag);
        }
        Ok(OsString::from(rest.strip_prefix('=').unwrap_or(rest)))
    }

    fn take_value(&self, args: &[OsString], i: &mut usize, flag: &str) -> io::Result<OsString> {
        let Some(value) = args.get(*i) else {
            return Err(invalid(format!(
                "the native CLI rejects this argv: {flag} requires a value"
            )));
        };
        let bytes = value.as_os_str().as_encoded_bytes();
        if bytes.len() > 1 && bytes.first() == Some(&b'-') {
            return Err(invalid(format!(
                "the native CLI rejects this argv: {flag} requires a value, and '{}' looks like another flag",
                value.to_string_lossy()
            )));
        }
        *i += 1;
        Ok(value.clone())
    }
}

fn finalize(session: &Session, cwd: &Path) -> io::Result<(Vec<OsString>, PathBuf)> {
    if session.approve_for_me && session.bypass_sandbox {
        return Err(unsupported(
            "--approve-for-me and --dangerously-bypass-approvals-and-sandbox are combined; the native precedence between them is not established",
        ));
    }
    if (session.approve_for_me || session.bypass_sandbox)
        && (session.sandbox.is_some() || session.approval.is_some())
    {
        return Err(unsupported(
            "explicit -s/--sandbox or -a/--ask-for-approval is combined with --approve-for-me or --dangerously-bypass-approvals-and-sandbox; the native precedence between them is not established",
        ));
    }
    if !session.add_dirs.is_empty()
        && session.config_values.iter().any(|value| {
            value.starts_with("sandbox_workspace_write.writable_roots=")
                || value.starts_with("sandbox_workspace_write=")
        })
    {
        return Err(unsupported(
            "--add-dir cannot be merged with an explicit sandbox_workspace_write writable-roots override; the combined root list for the backend is not established",
        ));
    }
    let directory = match &session.cd {
        Some(value) => {
            let path = Path::new(value);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.join(path)
            }
        }
        None => cwd.to_path_buf(),
    };
    let mut backend = session.backend_user.clone();
    if let Some(model) = &session.model {
        push_override(&mut backend, "model", &toml_string(model));
    }
    if session.approve_for_me {
        push_override(&mut backend, "approval_policy", &toml_string("on-request"));
        push_override(
            &mut backend,
            "sandbox_mode",
            &toml_string("workspace-write"),
        );
        push_override(
            &mut backend,
            "approvals_reviewer",
            &toml_string("auto_review"),
        );
    } else if session.bypass_sandbox {
        push_override(&mut backend, "approval_policy", &toml_string("never"));
        push_override(
            &mut backend,
            "sandbox_mode",
            &toml_string("danger-full-access"),
        );
    } else {
        if let Some(mode) = &session.sandbox {
            push_override(&mut backend, "sandbox_mode", &toml_string(mode));
        }
        if let Some(policy) = &session.approval {
            push_override(&mut backend, "approval_policy", &toml_string(policy));
        }
    }
    if !session.add_dirs.is_empty() {
        let roots = session
            .add_dirs
            .iter()
            .map(|dir| {
                let path = Path::new(dir);
                let resolved = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    directory.join(path)
                };
                toml_string(&resolved.to_string_lossy())
            })
            .collect::<Vec<_>>()
            .join(", ");
        push_override(
            &mut backend,
            "sandbox_workspace_write.writable_roots",
            &format!("[{roots}]"),
        );
    }
    Ok((backend, directory))
}

fn push_override(backend: &mut Vec<OsString>, key: &str, toml_value: &str) {
    backend.push("-c".into());
    backend.push(format!("{key}={toml_value}").into());
}

fn utf8_owned(value: OsString, flag: &str) -> io::Result<String> {
    value.into_string().map_err(|_| {
        invalid(format!(
            "the native CLI rejects this argv: {flag} requires a UTF-8 value"
        ))
    })
}

fn validate_override(value: &str) -> io::Result<()> {
    match value.split_once('=') {
        Some((key, _)) if !key.is_empty() => Ok(()),
        _ => Err(invalid(format!(
            "the native CLI rejects the -c override '{value}': expected key=value"
        ))),
    }
}

fn validate_enum(value: &str, flag: &str, possible: &[&str]) -> io::Result<String> {
    if possible.contains(&value) {
        Ok(value.to_owned())
    } else {
        Err(invalid(format!(
            "invalid value '{value}' for {flag}; possible values: {}",
            possible.join(", ")
        )))
    }
}

fn reject_attached(name: &str, attached: Option<&str>) -> io::Result<()> {
    if let Some(value) = attached {
        return Err(invalid(format!(
            "the native CLI rejects this argv: --{name} does not take a value (got '{value}')"
        )));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, message.into())
}

fn unsupported(message: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::Unsupported, message.into())
}

/// Serializes one value as a TOML basic string, as the "-c" parser expects.
fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => {
                out.push('\\');
                out.push('"');
            }
            '\\' => {
                out.push('\\');
                out.push('\\');
            }
            '\n' => {
                out.push('\\');
                out.push('n');
            }
            '\r' => {
                out.push('\\');
                out.push('r');
            }
            '\t' => {
                out.push('\\');
                out.push('t');
            }
            ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04X}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn plan_managed(list: &[&str]) -> ManagedArguments {
        let args = argv(list);
        plan(&args, Path::new("D:/base"))
            .expect("ordinary session arguments should plan")
            .expect("ordinary session arguments should be managed")
    }

    fn bypasses(list: &[&str]) {
        assert!(
            plan(&argv(list), Path::new("D:/base"))
                .expect("bypass cases must not be argv errors")
                .is_none(),
            "{list:?} should keep its native bypass"
        );
    }

    fn error_kind(list: &[&str]) -> ErrorKind {
        plan(&argv(list), Path::new("D:/base"))
            .expect_err(&format!("{list:?} should be rejected"))
            .kind()
    }

    #[test]
    fn plain_and_prompt_sessions_are_managed() {
        let planned = plan_managed(&[]);
        assert!(planned.backend.is_empty());
        assert!(planned.tui.is_empty());
        assert_eq!(planned.cwd, Path::new("D:/base"));

        let planned = plan_managed(&["fix the --profile flag"]);
        assert!(planned.backend.is_empty());
        assert_eq!(planned.tui, argv(&["fix the --profile flag"]));
    }

    #[test]
    fn second_prompt_word_is_rejected_like_the_native_cli() {
        assert_eq!(error_kind(&["hello", "world"]), ErrorKind::InvalidInput);
    }

    #[test]
    fn session_flags_map_to_config_overrides() {
        let planned = plan_managed(&[
            "-s",
            "workspace-write",
            "-m",
            "gpt-x",
            "-a",
            "never",
            "do the task",
        ]);
        assert_eq!(
            planned.backend,
            argv(&[
                "-c",
                "model=\"gpt-x\"",
                "-c",
                "sandbox_mode=\"workspace-write\"",
                "-c",
                "approval_policy=\"never\"",
            ])
        );
        assert_eq!(planned.cwd, Path::new("D:/base"));
    }

    #[test]
    fn user_overrides_keep_order_and_mapped_flags_follow() {
        let planned = plan_managed(&[
            "-c",
            "a=1",
            "--enable",
            "flag_x",
            "-c",
            "model=\"base\"",
            "--disable",
            "flag_y",
            "--strict-config",
            "-m",
            "gpt-x",
            "hello",
        ]);
        assert_eq!(
            planned.backend,
            argv(&[
                "-c",
                "a=1",
                "--enable",
                "flag_x",
                "-c",
                "model=\"base\"",
                "--disable",
                "flag_y",
                "--strict-config",
                "-c",
                "model=\"gpt-x\"",
            ])
        );
    }

    #[test]
    fn approve_for_me_maps_documented_equivalence() {
        let planned = plan_managed(&["--approve-for-me", "hello"]);
        assert_eq!(
            planned.backend,
            argv(&[
                "-c",
                "approval_policy=\"on-request\"",
                "-c",
                "sandbox_mode=\"workspace-write\"",
                "-c",
                "approvals_reviewer=\"auto_review\"",
            ])
        );
    }

    #[test]
    fn bypass_sandbox_maps_never_and_danger_full_access() {
        let planned = plan_managed(&["--dangerously-bypass-approvals-and-sandbox", "hello"]);
        assert_eq!(
            planned.backend,
            argv(&[
                "-c",
                "approval_policy=\"never\"",
                "-c",
                "sandbox_mode=\"danger-full-access\"",
            ])
        );
    }

    #[test]
    fn conflicting_full_auto_flags_are_reported() {
        for list in [
            vec![
                "--dangerously-bypass-approvals-and-sandbox",
                "-s",
                "read-only",
                "hi",
            ],
            vec!["--approve-for-me", "-a", "never", "hi"],
            vec![
                "--approve-for-me",
                "--dangerously-bypass-approvals-and-sandbox",
                "hi",
            ],
        ] {
            let error = plan(&argv(&list), Path::new("D:/base"))
                .expect_err(&format!("{list:?} should report its unclear precedence"));
            assert_eq!(error.kind(), ErrorKind::Unsupported);
            assert!(
                error.to_string().contains("precedence"),
                "the error should name the unclear precedence: {error}"
            );
        }
    }

    #[test]
    fn cd_resolves_against_cwd_and_add_dirs_map_to_roots() {
        let planned = plan_managed(&[
            "-C",
            "proj",
            "--add-dir",
            "data",
            "--add-dir",
            "D:/abs",
            "hello",
        ]);
        assert_eq!(planned.cwd, Path::new("D:/base").join("proj"));
        let data = toml_string(
            &Path::new("D:/base")
                .join("proj")
                .join("data")
                .to_string_lossy(),
        );
        assert_eq!(
            planned.backend,
            argv(&[
                "-c",
                &format!("sandbox_workspace_write.writable_roots=[{data}, \"D:/abs\"]")
            ])
        );
    }

    #[test]
    fn absolute_cd_wins_and_backend_stays_free_of_cd() {
        let planned = plan_managed(&["-C", "D:/elsewhere", "--cd=D:/final", "hello"]);
        assert_eq!(planned.cwd, Path::new("D:/final"));
        assert!(planned.backend.is_empty());
    }

    #[test]
    fn add_dir_conflicting_with_explicit_roots_override_is_reported() {
        let error = plan(
            &argv(&[
                "-c",
                "sandbox_workspace_write.writable_roots=[\"D:/x\"]",
                "--add-dir",
                "data",
                "hello",
            ]),
            Path::new("D:/base"),
        )
        .expect_err("merged root lists are not established");
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        assert!(
            error.to_string().contains("--add-dir"),
            "the error should name --add-dir: {error}"
        );
    }

    #[test]
    fn separator_keeps_option_looking_prompts_verbatim() {
        let planned = plan_managed(&["--", "--remote"]);
        assert!(planned.backend.is_empty());
        assert_eq!(planned.tui, argv(&["--", "--remote"]));

        let planned = plan_managed(&["-m", "gpt-x", "--", "--help"]);
        assert_eq!(planned.backend, argv(&["-c", "model=\"gpt-x\""]));
        assert_eq!(planned.tui, argv(&["-m", "gpt-x", "--", "--help"]));
    }

    #[test]
    fn image_values_stay_in_tui_and_hyphen_values_mirror_native() {
        let planned = plan_managed(&["-i", "a.png", "b.png", "-m", "gpt-x", "hello"]);
        assert_eq!(planned.backend, argv(&["-c", "model=\"gpt-x\""]));
        assert_eq!(
            planned.tui,
            argv(&["-i", "a.png", "b.png", "-m", "gpt-x", "hello"])
        );

        let planned = plan_managed(&["--image=-dash.png", "hello"]);
        assert!(planned.backend.is_empty());

        assert_eq!(
            error_kind(&["-i", "--dash.png", "hello"]),
            ErrorKind::InvalidInput
        );
    }

    #[test]
    fn explicit_remote_bypasses_management() {
        bypasses(&["--remote", "ws://127.0.0.1:1", "hello"]);
        bypasses(&["--remote=ws://127.0.0.1:1"]);
        bypasses(&["resume", "--remote", "ws://127.0.0.1:1"]);
    }

    #[test]
    fn explicit_profile_bypasses_management() {
        bypasses(&["-p", "work", "hello"]);
        bypasses(&["--profile", "work", "hello"]);
        bypasses(&["--profile=work", "hello"]);
        bypasses(&["-pwork", "hello"]);
    }

    #[test]
    fn help_and_version_bypass_management() {
        bypasses(&["--help"]);
        bypasses(&["-h"]);
        bypasses(&["-V"]);
        bypasses(&["--version"]);
        bypasses(&["-hV"]);
        bypasses(&["resume", "--help"]);
        bypasses(&["hello", "--version"]);
    }

    #[test]
    fn noninteractive_subcommands_bypass_management() {
        for list in [
            vec!["exec", "do work"],
            vec!["e", "do work"],
            vec!["review", "."],
            vec!["app-server"],
            vec!["mcp-server"],
            vec!["mcp", "list"],
            vec!["agents"],
            vec!["cloud"],
            vec!["features", "list"],
            vec!["debug", "prompt-input"],
            vec!["help"],
        ] {
            bypasses(&list);
        }
    }

    #[test]
    fn resume_and_fork_sessions_are_managed() {
        let planned = plan_managed(&["resume"]);
        assert!(planned.backend.is_empty());
        assert_eq!(planned.cwd, Path::new("D:/base"));

        let planned = plan_managed(&["resume", "--last"]);
        assert_eq!(planned.tui, argv(&["resume", "--last"]));

        let planned = plan_managed(&["fork", "9b2f", "continue here"]);
        assert!(planned.backend.is_empty());

        let planned = plan_managed(&["-c", "a=1", "resume", "--all", "9b2f"]);
        assert_eq!(planned.backend, argv(&["-c", "a=1"]));

        assert_eq!(
            error_kind(&["resume", "a", "b", "c"]),
            ErrorKind::InvalidInput
        );
    }

    #[test]
    fn native_argument_errors_are_mirrored() {
        for list in [
            vec!["-c"],
            vec!["--config"],
            vec!["-c", "missing-equals"],
            vec!["-m"],
            vec!["-s"],
            vec!["--enable"],
            vec!["-s", "fast"],
            vec!["-a", "always"],
            vec!["-a", "on-failure"],
            vec!["--wat", "hello"],
            vec!["-z", "hello"],
            vec!["-m", "-x", "hello"],
            vec!["--worktree=yes", "hello"],
        ] {
            assert_eq!(error_kind(&list), ErrorKind::InvalidInput, "{list:?}");
        }
    }

    #[test]
    fn flags_without_established_backend_effect_are_reported() {
        for list in [
            vec!["--oss", "hello"],
            vec!["--search", "hello"],
            vec!["--local-provider", "ollama", "hello"],
            vec!["--dangerously-bypass-hook-trust", "hello"],
        ] {
            let error = plan(&argv(&list), Path::new("D:/base"))
                .expect_err(&format!("{list:?} should report its uncertain mapping"));
            assert_eq!(error.kind(), ErrorKind::Unsupported, "{list:?}");
            let message = error.to_string();
            let flag = list[0];
            assert!(
                message.contains(flag),
                "the error should name {flag}: {message}"
            );
        }
    }

    #[test]
    fn attached_and_cluster_values_are_accepted() {
        let planned = plan_managed(&["-mgpt-x", "hello"]);
        assert_eq!(planned.backend, argv(&["-c", "model=\"gpt-x\""]));

        let planned = plan_managed(&["--model=gpt-x", "hello"]);
        assert_eq!(planned.backend, argv(&["-c", "model=\"gpt-x\""]));

        let planned = plan_managed(&["--config=x=1", "hello"]);
        assert_eq!(planned.backend, argv(&["-c", "x=1"]));

        let planned = plan_managed(&["-c=x=1", "hello"]);
        assert_eq!(planned.backend, argv(&["-c", "x=1"]));
    }

    #[test]
    fn tui_only_flags_do_not_reach_the_backend() {
        let planned = plan_managed(&[
            "--worktree",
            "--no-alt-screen",
            "--remote-auth-token-env",
            "TOKEN",
            "hello",
        ]);
        assert!(planned.backend.is_empty());
        assert_eq!(planned.tui.len(), 5);
    }

    #[test]
    fn attachment_keeps_permissions_on_backend_and_preserves_prompt_and_model() {
        let args = [
            "-c",
            "approval_policy=\"never\"",
            "--config=sandbox_mode=\"read-only\"",
            "-cmodel_reasoning_effort=\"low\"",
            "-m",
            "gpt-6-astra",
            "--no-alt-screen",
            "--",
            "--sandbox=workspace-write",
        ];
        let planned = plan_managed(&args);
        assert!(planned.new_session);
        assert_eq!(planned.tui, argv(&args));
        assert_eq!(
            planned.attachment,
            argv(&[
                "-cmodel_reasoning_effort=\"low\"",
                "-m",
                "gpt-6-astra",
                "--no-alt-screen",
                "--",
                "--sandbox=workspace-write"
            ])
        );
        assert!(
            planned
                .backend
                .contains(&"approval_policy=\"never\"".into())
        );
        assert!(
            planned
                .backend
                .contains(&"sandbox_mode=\"read-only\"".into())
        );
        let planned = plan_managed(&["-sread-only", "-anever", "--add-dir=D:/owned", "hello"]);
        assert_eq!(planned.attachment, argv(&["hello"]));
        assert!(
            planned
                .backend
                .contains(&"sandbox_mode=\"read-only\"".into())
        );
        assert!(
            planned
                .backend
                .contains(&"sandbox_workspace_write.writable_roots=[\"D:/owned\"]".into())
        );
    }

    #[test]
    fn existing_remote_resume_does_not_discard_requested_permissions() {
        let error = plan(
            &argv(&["resume", "--last", "-sread-only"]),
            Path::new("D:/base"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }

    #[test]
    fn attachment_handles_config_equals_and_does_not_remove_image_values() {
        let planned = plan_managed(&[
            "-c=approval_policy=\"never\"",
            "-i",
            "approval_policy=never.png",
            "--",
            "--ask-for-approval=never",
        ]);
        assert_eq!(
            planned.attachment,
            argv(&[
                "-i",
                "approval_policy=never.png",
                "--",
                "--ask-for-approval=never"
            ])
        );
        assert_eq!(planned.backend, argv(&["-c", "approval_policy=\"never\""]));
    }

    #[test]
    fn toml_strings_escape_windows_paths_and_quotes() {
        assert_eq!(toml_string("D:\\dir\\sub"), "\"D:\\\\dir\\\\sub\"");
        assert_eq!(toml_string("a\"b"), "\"a\\\"b\"");
    }
}
