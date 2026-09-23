//! GitHub release redirect admission for retained native dependencies.
#![cfg(windows)]

pub(crate) struct Asset {
    pub url: String,
    /// Retained for callers that verify archive identity after download.
    #[allow(dead_code)]
    pub sha256: String,
    pub size: u64,
    #[allow(dead_code)]
    pub id: u64,
}

pub(crate) fn admitted_redirect(url: &str) -> bool {
    // Repo ids were independently confirmed via the connected GitHub
    // repository API. Retain these identities if similarly named repositories
    // are recreated.
    const PREFIX: &str =
        "https://release-assets.githubusercontent.com/github-production-release-asset/";
    const RTK_REPOSITORY_ID: &str = "1139971460";
    const BEADS_REPOSITORY_ID: &str = "1074561042";
    let Some(rest) = url.strip_prefix(PREFIX) else {
        return false;
    };
    let Some((repo, tail)) = rest.split_once('/') else {
        return false;
    };
    if repo != RTK_REPOSITORY_ID && repo != BEADS_REPOSITORY_ID {
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

    #[test]
    fn redirect_admission_is_one_exact_cdn_repository_namespace() {
        let valid = "https://release-assets.githubusercontent.com/github-production-release-asset/1139971460/b55d0525-c35f-4a50-9ca1-9ac99cc1eb2a?sp=r&sig=PRIVATE-TOKEN";
        assert!(admitted_redirect(valid));
        for invalid in [
            valid.replace("1139971460", "123"),
            valid.replace("1139971460", "1166102148"),
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
        let beads = valid.replace("1139971460", "1074561042");
        assert!(admitted_redirect(&beads));
    }
}
