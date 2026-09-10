//! The managed argument contract, independent of upstream catalogue filtering.
use serde_json::{Map, Value, json};
use std::{io, time::Duration};

pub const INSTRUCTIONS: &str = "CodeGraph 1.6.0 managed adapter. Select the exact project root in the connection. Use codegraph_index deliberately for an initial/full index and codegraph_sync for an explicit incremental checkpoint. Shared connections observe every active indexed project and fairly queue automatic catch-up under one account indexing slot. Each episode is limited to 600 seconds; healthy worker retirement preserves observation. After an actual failure inspect the cause and sync deliberately. Prefer Serena for known-file symbols, exact references and edits. Graph edges are candidates. Pending, failed or unverified coverage requires current source. Five matches, depth one and 4096 serialized bytes by default; explore requires an explicit question and one or two files. Details are private to this client and expire; retrieving them never repeats a query.";

pub struct Request {
    pub arguments: Value,
    pub max_bytes: usize,
    pub timeout: Duration,
}

fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}

fn integer(
    args: &mut Map<String, Value>,
    key: &str,
    default: u64,
    min: u64,
    max: u64,
) -> io::Result<()> {
    let value = args.entry(key).or_insert_with(|| json!(default));
    if !value.as_u64().is_some_and(|n| (min..=max).contains(&n)) {
        return Err(invalid(&format!(
            "{key} must be an integer in {min}..={max}"
        )));
    }
    Ok(())
}

fn string(args: &Map<String, Value>, key: &str, required: bool) -> io::Result<()> {
    match args.get(key) {
        None if !required => Ok(()),
        Some(Value::String(s)) if !s.trim().is_empty() && s.len() <= 1024 && !s.contains('\0') => {
            Ok(())
        }
        _ => Err(invalid(&format!(
            "{key} must be a nonempty string of at most 1024 bytes"
        ))),
    }
}

pub fn request(name: &str, value: Value) -> io::Result<Request> {
    let mut args = value
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("arguments must be an object"))?;
    integer(&mut args, "max_response_bytes", 4096, 1024, 16384)?;
    let max_bytes = args.remove("max_response_bytes").unwrap().as_u64().unwrap() as usize;
    let fields: &[&str] = match name {
        "codegraph_search" => &["query", "kind", "limit"],
        "codegraph_callers" | "codegraph_callees" => &["symbol", "file", "limit"],
        "codegraph_impact" => &["symbol", "file", "depth"],
        "codegraph_node" => &[
            "symbol",
            "file",
            "includeCode",
            "symbolsOnly",
            "offset",
            "limit",
            "line",
        ],
        "codegraph_explore" => &["query", "maxFiles"],
        "codegraph_status" | "codegraph_index" | "codegraph_sync" => &[],
        "codegraph_detail" => &["id", "offset"],
        _ => return Err(invalid("unsupported managed CodeGraph operation")),
    };
    if args.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err(invalid(
            "unknown argument; project selection belongs to this connection",
        ));
    }
    for key in ["query", "symbol", "file", "kind", "id"] {
        string(
            &args,
            key,
            matches!(
                (name, key),
                ("codegraph_search" | "codegraph_explore", "query")
                    | (
                        "codegraph_callers" | "codegraph_callees" | "codegraph_impact",
                        "symbol"
                    )
                    | ("codegraph_detail", "id")
            ),
        )?;
    }
    if let Some(kind) = args.get("kind").and_then(Value::as_str)
        && ![
            "function",
            "method",
            "class",
            "interface",
            "type",
            "variable",
            "route",
            "component",
        ]
        .contains(&kind)
    {
        return Err(invalid("unsupported symbol kind"));
    }
    if let Some(file) = args.get("file").and_then(Value::as_str)
        && (file.contains(['\\', ':'])
            || file.starts_with('/')
            || file
                .split('/')
                .any(|s| s.is_empty() || s == "." || s == ".."))
    {
        return Err(invalid(
            "file must be an exact repository-relative path using /",
        ));
    }
    for key in ["includeCode", "symbolsOnly"] {
        if args.get(key).is_some_and(|v| !v.is_boolean()) {
            return Err(invalid("source mode flags must be booleans"));
        }
    }
    match name {
        "codegraph_search" | "codegraph_callers" | "codegraph_callees" => {
            integer(&mut args, "limit", 5, 1, 50)?
        }
        "codegraph_impact" => integer(&mut args, "depth", 1, 1, 3)?,
        "codegraph_explore" => {
            if !args.contains_key("maxFiles") {
                return Err(invalid("explore requires explicit maxFiles (1 or 2)"));
            }
            integer(&mut args, "maxFiles", 1, 1, 2)?;
        }
        "codegraph_node" => {
            if args.contains_key("symbol") {
                if args.contains_key("offset")
                    || args.contains_key("limit")
                    || args.contains_key("symbolsOnly")
                {
                    return Err(invalid(
                        "symbol mode cannot use file-window or overview arguments",
                    ));
                }
                args.entry("includeCode").or_insert(json!(false));
                if args.contains_key("line") {
                    if !args.contains_key("file") {
                        return Err(invalid("line requires an exact file"));
                    }
                    integer(&mut args, "line", 1, 1, 1_000_000)?;
                }
            } else {
                string(&args, "file", true)?;
                if args.contains_key("line") || args.contains_key("includeCode") {
                    return Err(invalid("file mode cannot use symbol arguments"));
                }
                if args.get("symbolsOnly") == Some(&json!(true)) {
                    if args.contains_key("offset") || args.contains_key("limit") {
                        return Err(invalid("overview cannot use a source window"));
                    }
                } else {
                    integer(&mut args, "offset", 1, 1, 1_000_000)?;
                    integer(&mut args, "limit", 40, 1, 200)?;
                }
            }
        }
        "codegraph_detail" => integer(&mut args, "offset", 0, 0, 262144)?,
        _ => {}
    }
    Ok(Request {
        arguments: Value::Object(args),
        max_bytes,
        timeout: Duration::from_secs(match name {
            "codegraph_index" | "codegraph_sync" => 600,
            "codegraph_explore" => 60,
            _ => 30,
        }),
    })
}

pub fn tools() -> Vec<Value> {
    let s = || json!({"type":"string","minLength":1,"maxLength":1024});
    let n =
        |min, max, default| json!({"type":"integer","minimum":min,"maximum":max,"default":default});
    let definitions = [
        (
            "codegraph_search",
            "Find symbol locations; five matches by default.",
            json!({"query":s(),"kind":{"enum":["function","method","class","interface","type","variable","route","component"]},"limit":n(1,50,5)}),
            vec!["query"],
        ),
        (
            "codegraph_callers",
            "Direct caller candidates. Use an exact file to disambiguate.",
            json!({"symbol":s(),"file":s(),"limit":n(1,50,5)}),
            vec!["symbol"],
        ),
        (
            "codegraph_callees",
            "Direct callee candidates.",
            json!({"symbol":s(),"file":s(),"limit":n(1,50,5)}),
            vec!["symbol"],
        ),
        (
            "codegraph_impact",
            "Shallow impact candidates, depth one by default.",
            json!({"symbol":s(),"file":s(),"depth":n(1,3,1)}),
            vec!["symbol"],
        ),
        (
            "codegraph_node",
            "Symbol signature/body, file overview, or bounded source window. Exact relative file required for file mode.",
            json!({"symbol":s(),"file":s(),"includeCode":{"type":"boolean","default":false},"symbolsOnly":{"type":"boolean","default":false},"offset":n(1,1_000_000,1),"limit":n(1,200,40),"line":n(1,1_000_000,1)}),
            vec![],
        ),
        (
            "codegraph_explore",
            "Explicit cross-file question; maxFiles must be 1 or 2. Source count is not a byte budget.",
            json!({"query":s(),"maxFiles":{"type":"integer","minimum":1,"maximum":2}}),
            vec!["query", "maxFiles"],
        ),
        (
            "codegraph_status",
            "Inspect the selected root and upstream index status; coverage may remain unverified.",
            json!({}),
            vec![],
        ),
        (
            "codegraph_index",
            "Deliberately create/rebuild this exact root's owned index under bounded resources; preserve the last committed checkpoint on failure.",
            json!({}),
            vec![],
        ),
        (
            "codegraph_sync",
            "Deliberately catch up from the last saved checkpoint, including after a worker failure; commit only after bounded completion.",
            json!({}),
            vec![],
        ),
        (
            "codegraph_detail",
            "Read a retained response page without repeating its query. IDs expire after 30 minutes or eviction; offsets are UTF-8 bytes.",
            json!({"id":s(),"offset":n(0,262144,0)}),
            vec!["id"],
        ),
    ];
    definitions.into_iter().map(|(name,description,mut properties,required)| {
        properties["max_response_bytes"] = n(1024,16384,4096);
        json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_defaults_and_explicit_breadth() {
        assert_eq!(
            request("codegraph_search", json!({"query":"entry"}))
                .unwrap()
                .arguments["limit"],
            5
        );
        assert_eq!(
            request("codegraph_impact", json!({"symbol":"entry"}))
                .unwrap()
                .arguments["depth"],
            1
        );
        assert!(request("codegraph_explore", json!({"query":"entry"})).is_err());
        assert!(request("codegraph_explore", json!({"query":"entry","maxFiles":3})).is_err());
        assert_eq!(
            request("codegraph_node", json!({"file":"src/main.rs"}))
                .unwrap()
                .arguments["limit"],
            40
        );
    }
    #[test]
    fn rejects_hidden_overrides_and_invalid_modes() {
        for args in [
            json!({"query":"x","limit":1.5}),
            json!({"query":"x","max_response_bytes":16385}),
            json!({"query":"x","projectPath":"elsewhere"}),
            json!({"query":"x","limit":0}),
            json!({"query":"x","kind":"invalid"}),
        ] {
            assert!(request("codegraph_search", args).is_err());
        }
        for args in [
            json!({"file":"../secret"}),
            json!({"file":"C:/secret"}),
            json!({"file":"a.rs","symbol":"x","limit":5}),
            json!({"file":"a.rs","symbolsOnly":true,"offset":3}),
            json!({"symbol":"x","line":4}),
        ] {
            assert!(request("codegraph_node", args).is_err());
        }
    }
}
