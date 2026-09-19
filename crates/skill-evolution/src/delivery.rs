//! Bounded hooks-off catalogue delta. Ordinary hooks stay off.

use crate::invalid;
use std::io;

const LIMIT: usize = 2048;

pub fn compact(name: &str, applicability: &str, path: &str, revision: &str) -> io::Result<String> {
    for field in [name, applicability, path, revision] {
        if field.len() > LIMIT
            || field.contains('\0')
            || field.contains('<')
            || field.contains('>')
            || field.contains('\n')
        {
            return Err(invalid("malformed or injected catalogue field"));
        }
    }
    if name.is_empty() || path.is_empty() || revision.is_empty() {
        return Err(invalid("incomplete catalogue identity"));
    }
    Ok(format!(
        "name={name};applicability={applicability};path={path};revision={revision}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_and_injected_fields_are_incomplete_not_hook_calls() {
        assert!(compact("demo", "when useful", "skills/demo", "abc").is_ok());
        assert!(compact("demo\ninject", "x", "p", "r").is_err());
        assert!(compact("<script>", "x", "p", "r").is_err());
        assert!(compact("demo", "x", "", "r").is_err());
    }
}
