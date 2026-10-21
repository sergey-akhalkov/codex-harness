//! Explicit local report formatting and retained-policy evaluation; never
//! starts a process, model or hook.
use harness_core::{
    improvement_policy::{ComparisonPolicy, DeclaredComparison, evaluate},
    outcome_report,
};
use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;

enum Mode {
    Summarize {
        input: PathBuf,
        markdown: bool,
    },
    Declare {
        policy: PathBuf,
    },
    Evaluate {
        input: PathBuf,
        declaration: PathBuf,
    },
}

fn usage() -> io::Error {
    invalid(
        "outcome-report requires --input PATH [--markdown], --declare-policy FILE, or --input PATH --policy DECLARATION",
    )
}

fn option_value(iter: &mut impl Iterator<Item = OsString>, flag: &str) -> io::Result<PathBuf> {
    let value = iter
        .next()
        .ok_or_else(|| invalid(format!("{flag} requires a file path")))?;
    if matches!(
        value.to_str(),
        Some("--input" | "--policy" | "--declare-policy" | "--markdown" | "--help")
    ) {
        return Err(invalid(format!("{flag} requires a file path")));
    }
    Ok(PathBuf::from(value))
}

fn parse_mode(args: &[OsString]) -> io::Result<Mode> {
    let mut input = None;
    let mut declaration = None;
    let mut declare_policy = None;
    let mut markdown = false;
    let mut iter = args.iter().cloned();
    while let Some(arg) = iter.next() {
        if arg.to_str() == Some("--input") && input.is_none() {
            input = Some(option_value(&mut iter, "--input")?);
        } else if arg.to_str() == Some("--policy") && declaration.is_none() {
            declaration = Some(option_value(&mut iter, "--policy")?);
        } else if arg.to_str() == Some("--declare-policy") && declare_policy.is_none() {
            declare_policy = Some(option_value(&mut iter, "--declare-policy")?);
        } else if arg.to_str() == Some("--markdown") && !markdown {
            markdown = true;
        } else {
            return Err(usage());
        }
    }
    match (declare_policy, declaration, input) {
        (Some(policy), None, None) if !markdown => Ok(Mode::Declare { policy }),
        (None, Some(declaration), Some(input)) if !markdown => {
            Ok(Mode::Evaluate { input, declaration })
        }
        (None, None, Some(input)) => Ok(Mode::Summarize { input, markdown }),
        _ => Err(usage()),
    }
}

fn read_bounded(path: &Path, kind: &str) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_JSON_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(invalid(format!(
            "the {kind} exceeds the {MAX_JSON_BYTES}-byte JSON bound"
        )));
    }
    Ok(bytes)
}

fn read_input(path: &Path) -> io::Result<serde_json::Value> {
    let bytes = read_bounded(path, "input")?;
    serde_json::from_slice(&bytes)
        .map_err(|_| invalid("the input must be a bounded valid attempt array or report"))
}

fn summarize_attempts_in(value: &serde_json::Value) -> io::Result<serde_json::Value> {
    let attempts = value
        .as_array()
        .or_else(|| value.get("attempts").and_then(|value| value.as_array()))
        .ok_or_else(|| invalid("the input must be a bounded valid attempt array or report"))?;
    outcome_report::summarize_attempts(attempts)
}

fn summarize_input(path: &Path) -> io::Result<serde_json::Value> {
    summarize_attempts_in(&read_input(path)?)
}

fn evaluation_report(path: &Path) -> io::Result<serde_json::Value> {
    let value = read_input(path)?;
    let report = summarize_attempts_in(&value)?;
    match value.get("corroboration") {
        Some(corroboration) => attach_retained_corroboration(report, corroboration),
        None => Ok(report),
    }
}

fn attach_retained_corroboration(
    report: serde_json::Value,
    corroboration: &serde_json::Value,
) -> io::Result<serde_json::Value> {
    let section: outcome_report::CorroborationSection =
        serde_json::from_value(corroboration.clone())
            .map_err(|_| invalid("the report carries an invalid corroboration section"))?;
    if section.schema != outcome_report::CORROBORATION_SCHEMA {
        return Err(invalid(
            "the report corroboration section has an unsupported schema",
        ));
    }
    if outcome_report::corroboration_digest(&section)? != section.digest {
        return Err(invalid(
            "the report corroboration section does not bind its own content",
        ));
    }
    let receipt = retained_corroboration_receipt(&section)?;
    outcome_report::attach_corroboration(&report, &receipt)
}

fn retained_corroboration_receipt(
    section: &outcome_report::CorroborationSection,
) -> io::Result<serde_json::Value> {
    let selection = match section.status {
        outcome_report::CorroborationState::Ready => {
            if !section.ready || section.reason.is_some() {
                return Err(invalid(
                    "the report corroboration ready state contradicts its reason",
                ));
            }
            serde_json::json!({
                "schema": 1,
                "requiredUnits": section.required_units,
                "status": "ready",
                "units": &section.units,
                "excluded": &section.excluded,
            })
        }
        outcome_report::CorroborationState::Inconclusive => {
            if section.ready {
                return Err(invalid(
                    "the report corroboration inconclusive state claims readiness",
                ));
            }
            let reason = section.reason.as_deref().ok_or_else(|| {
                invalid("the report corroboration inconclusive state has no reason")
            })?;
            serde_json::json!({
                "schema": 1,
                "requiredUnits": section.required_units,
                "status": {"inconclusive": reason},
                "units": &section.units,
                "excluded": &section.excluded,
            })
        }
        outcome_report::CorroborationState::Unavailable => {
            if section.ready || !section.units.is_empty() || !section.excluded.is_empty() {
                return Err(invalid(
                    "the report corroboration unavailable state carries selected units",
                ));
            }
            let reason = section.reason.as_deref().ok_or_else(|| {
                invalid("the report corroboration unavailable state has no reason")
            })?;
            return Ok(serde_json::json!({
                "schema": 1,
                "status": "unavailable",
                "required_units": section.required_units,
                "selection": serde_json::Value::Null,
                "reason": reason,
            }));
        }
    };
    Ok(serde_json::json!({
        "schema": 1,
        "status": "selected",
        "required_units": section.required_units,
        "selection": selection,
        "reason": serde_json::Value::Null,
    }))
}

fn emit(result: String) -> io::Result<i32> {
    if result.len() > MAX_OUTPUT_BYTES {
        return Err(invalid("the report exceeds the 32 MiB output bound"));
    }
    print!("{result}");
    Ok(0)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    if args == [OsString::from("--help")] {
        println!(
            "codex-harness outcome-report --input PATH [--markdown]\n\
             codex-harness outcome-report --declare-policy FILE\n\
             codex-harness outcome-report --input PATH --policy DECLARATION\n\
             Reads private local JSON (up to 8 MiB, 1024 attempts). Declaration prints the \
             digested DeclaredComparison; evaluation prints the PolicyEvaluation for the supplied \
             retained evidence. No model calls; the caller owns declaration chronology and adoption."
        );
        return Ok(0);
    }
    match parse_mode(args)? {
        Mode::Declare { policy } => {
            let policy: ComparisonPolicy =
                serde_json::from_slice(&read_bounded(&policy, "policy")?).map_err(|_| {
                    invalid("the policy must be a bounded valid ComparisonPolicy JSON document")
                })?;
            let declared = policy.declare()?;
            emit(format!("{}\n", serde_json::to_string_pretty(&declared)?))
        }
        Mode::Evaluate { input, declaration } => {
            let declared: DeclaredComparison = serde_json::from_slice(&read_bounded(
                &declaration,
                "policy declaration",
            )?)
            .map_err(|_| {
                invalid("the declaration must be a bounded valid DeclaredComparison JSON document")
            })?;
            declared.verify()?;
            let report = evaluation_report(&input)?;
            let evaluation = evaluate(&declared, &report)?;
            emit(format!("{}\n", serde_json::to_string_pretty(&evaluation)?))
        }
        Mode::Summarize { input, markdown } => {
            let report = summarize_input(&input)?;
            let result = if markdown {
                outcome_report::concise_report(&report)?
            } else {
                format!("{}\n", serde_json::to_string_pretty(&report)?)
            };
            emit(result)
        }
    }
}
