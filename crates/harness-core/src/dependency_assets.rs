//! Exact public Codebase Memory native-release identity and redirect admission.
#![cfg(windows)]
use serde_json::Value;

pub(crate) struct Asset {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub id: u64,
}

pub(crate) fn metadata_url(version: &str) -> Result<String, &'static str> {
    crate::dependency_audit::endpoints("codebase-memory-mcp", version)
        .map_err(|_| "unsupported-metadata-source")?;
    Ok(format!(
        "https://api.github.com/repos/DeusData/codebase-memory-mcp/releases/tags/v{version}"
    ))
}

pub(crate) fn select(
    bytes: &[u8],
    version: &str,
    architecture: &str,
) -> Result<Asset, &'static str> {
    metadata_url(version)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("metadata-output-too-large");
    }
    let release: Value = serde_json::from_slice(bytes).map_err(|_| "unsupported-native-release")?;
    let architecture = match architecture {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return Err("unsupported-native-platform"),
    };
    let name = format!("codebase-memory-mcp-windows-{architecture}.zip");
    let url = format!(
        "https://github.com/DeusData/codebase-memory-mcp/releases/download/v{version}/{name}"
    );
    if release["tag_name"] != format!("v{version}")
        || release["draft"] != false
        || release["prerelease"] != false
    {
        return Err("unsupported-native-release");
    }
    let mut matches = release["assets"]
        .as_array()
        .ok_or("unsupported-native-release")?
        .iter()
        .filter(|asset| asset["name"] == name);
    let asset = matches.next().ok_or("native-asset-unavailable")?;
    let digest = asset["digest"]
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .ok_or("native-asset-digest-unavailable")?;
    let size = asset["size"].as_u64().ok_or("unsupported-native-release")?;
    let id = asset["id"].as_u64().ok_or("unsupported-native-release")?;
    if matches.next().is_some()
        || asset["state"] != "uploaded"
        || asset["browser_download_url"] != url
        || id == 0
        || size == 0
        || size > 128 * 1024 * 1024
        || digest.len() != 64
        || !digest.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("unsupported-native-release");
    }
    Ok(Asset {
        url,
        sha256: digest.to_ascii_lowercase(),
        size,
        id,
    })
}

pub(crate) fn admitted_redirect(url: &str) -> bool {
    // Repo id was independently confirmed via the connected GitHub repository
    // API. Retain this identity if a similarly named repository is recreated.
    const PREFIX: &str =
        "https://release-assets.githubusercontent.com/github-production-release-asset/";
    let Some(rest) = url.strip_prefix(PREFIX) else {
        return false;
    };
    let Some((repo, tail)) = rest.split_once('/') else {
        return false;
    };
    if repo != "1166102148"
        && repo != crate::dependency_discovery::dependency_codegraph::REPOSITORY_ID
    {
        return false;
    }
    if url.len() > 16 * 1024
        || !url.is_ascii()
        || url
            .bytes()
            .any(|b| b <= b' ' || b == 127 || b == b'\\' || b == b'#')
    {
        return false;
    }
    let Some((uuid, query)) = tail.split_once('?') else {
        return false;
    };
    !query.is_empty()
        && uuid.len() == 36
        && uuid.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exact_native_asset_selection_rejects_substitution_and_ambiguity() {
        let version = "0.10.8";
        let mut release = json!({"tag_name":"v0.10.8","draft":false,"prerelease":false,"assets":[{
            "name":"codebase-memory-mcp-windows-amd64.zip","id":123,"size":100,"state":"uploaded",
            "digest":format!("sha256:{}","a".repeat(64)),"browser_download_url":"https://github.com/DeusData/codebase-memory-mcp/releases/download/v0.10.8/codebase-memory-mcp-windows-amd64.zip"
        }]});
        assert!(select(&serde_json::to_vec(&release).unwrap(), version, "x86_64").is_ok());
        assert!(select(&serde_json::to_vec(&release).unwrap(), "0.10.9", "x86_64").is_err());
        assert!(select(&serde_json::to_vec(&release).unwrap(), version, "aarch64").is_err());
        let asset = release["assets"][0].clone();
        release["assets"].as_array_mut().unwrap().push(asset);
        assert!(select(&serde_json::to_vec(&release).unwrap(), version, "x86_64").is_err());
        release["assets"].as_array_mut().unwrap().pop();
        release["assets"][0]["digest"] = json!(null);
        assert!(select(&serde_json::to_vec(&release).unwrap(), version, "x86_64").is_err());
    }

    #[test]
    fn redirect_admission_is_one_exact_cdn_repository_namespace() {
        let valid = "https://release-assets.githubusercontent.com/github-production-release-asset/1166102148/b55d0525-c35f-4a50-9ca1-9ac99cc1eb2a?sp=r&sig=PRIVATE-TOKEN";
        assert!(admitted_redirect(valid));
        for invalid in [
            valid.replace("1166102148", "123"),
            valid.replace("https://", "http://"),
            valid.replace(
                "release-assets.githubusercontent.com",
                "PRIVATE@release-assets.githubusercontent.com",
            ),
            valid.replace(".com/", ".com:444/"),
            format!("{valid}#fragment"),
            format!("{valid}\nheader"),
            valid.replace("b55d0525-c35f-4a50-9ca1-9ac99cc1eb2a", "../other"),
        ] {
            assert!(!admitted_redirect(&invalid));
        }
        let codegraph = valid.replace("1166102148", "1137078255");
        assert!(admitted_redirect(&codegraph));
    }
}
