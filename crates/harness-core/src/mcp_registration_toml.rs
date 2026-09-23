//! Byte-preserving statement boundaries for already validated TOML.
//! Header-looking text inside strings, arrays and comments is never a table.
use std::{collections::BTreeSet, io, ops::Range};

fn invalid() -> io::Error {
    io::Error::other("Cannot isolate owned TOML statements; preserving configuration.")
}

fn statements(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut start = usize::from(text.starts_with('\u{feff}')) * 3;
    let mut index = start;
    let mut quote = None;
    let mut multiline = false;
    let (mut square, mut curly) = (0i32, 0i32);
    let mut comment = false;
    let mut ranges = Vec::new();
    while index < bytes.len() {
        let byte = bytes[index];
        if comment {
            if !matches!(byte, b'\r' | b'\n') {
                index += 1;
                continue;
            }
            comment = false;
        }
        if let Some(delimiter) = quote {
            if delimiter == b'"' && byte == b'\\' {
                index += 2;
                continue;
            }
            if multiline && bytes[index..].starts_with(&[delimiter; 3]) {
                while index < bytes.len() && bytes[index] == delimiter {
                    index += 1;
                }
                quote = None;
                multiline = false;
                continue;
            }
            if !multiline && byte == delimiter {
                quote = None;
            }
            index += 1;
            continue;
        }
        match byte {
            b'#' => comment = true,
            b'"' | b'\'' => {
                quote = Some(byte);
                multiline = bytes[index..].starts_with(&[byte; 3]);
                if multiline {
                    index += 3;
                    continue;
                }
            }
            b'[' => square += 1,
            b']' => square -= 1,
            b'{' => curly += 1,
            b'}' => curly -= 1,
            _ => {}
        }
        if byte == b'\n' && square == 0 && curly == 0 {
            ranges.push(start..index + 1);
            start = index + 1;
        }
        index += 1;
    }
    if start < bytes.len() {
        ranges.push(start..bytes.len());
    }
    ranges
}

fn assignment_key(statement: &str) -> io::Result<&str> {
    let bytes = statement.as_bytes();
    let mut index = 0;
    let mut quote = None;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(delimiter) = quote {
            if delimiter == b'"' && byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == delimiter {
                quote = None;
            }
        } else if matches!(byte, b'"' | b'\'') {
            quote = Some(byte);
        } else if byte == b'=' {
            return Ok(&statement[..index]);
        }
        index += 1;
    }
    Err(invalid())
}

fn key_path(statement: &str) -> io::Result<Vec<String>> {
    let mut value: toml::Value = toml::from_str(statement).map_err(|_| invalid())?;
    let mut path = Vec::new();
    loop {
        match value {
            toml::Value::Table(table) if table.len() == 1 => {
                let (key, child) = table.into_iter().next().unwrap();
                path.push(key);
                value = child;
            }
            toml::Value::Array(mut array) if array.len() == 1 => value = array.remove(0),
            _ => return Ok(path),
        }
    }
}

pub(super) fn remove(
    bytes: &[u8],
    owned: &BTreeSet<String>,
    root_key: Option<&str>,
) -> io::Result<Vec<u8>> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut table = Vec::new();
    let mut output = Vec::with_capacity(bytes.len());
    let mut copied = 0;
    for range in statements(text) {
        let statement = text[range.clone()].trim();
        let remove = if !owned.is_empty()
            && matches!(
                statement,
                "# BEGIN codex-harness MCP registrations" | "# END codex-harness MCP registrations"
            ) {
            true
        } else if statement.is_empty() || statement.starts_with('#') {
            false
        } else {
            let is_table = statement.starts_with('[');
            let path = if is_table {
                table = key_path(statement)?;
                table.clone()
            } else {
                let mut path = table.clone();
                path.extend(key_path(&format!("{}= 0", assignment_key(statement)?))?);
                path
            };
            (path.len() >= 2 && path[0] == "mcp_servers" && owned.contains(&path[1]))
                || (!is_table && path.len() == 1 && root_key.is_some_and(|key| path[0] == key))
        };
        if remove {
            output.extend_from_slice(&bytes[copied..range.start]);
            copied = range.end;
        }
    }
    output.extend_from_slice(&bytes[copied..]);
    Ok(output)
}
