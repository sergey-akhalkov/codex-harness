//! Exact native broker receipts in a protected root. Stale receipt cleanup is
//! conditional on a free instance lock and proof of a dead/reused process ID.
#![cfg(windows)]
use crate::{
    broker_state::BrokerRoot,
    build_identity,
    dependency_mcp_probe::strict_json,
    process::ProcessIdentity,
    process_service::{ServiceProcess, current_user},
    registration_native::{FileGuard, ReadGuard, StagedFile, validate_local_path},
    resource_admission::Lease,
};
use serde::{Deserialize, Serialize};
use std::{fmt, io, path::PathBuf};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};

const FILE: &str = "endpoint.json";
const SCHEMA: &str = "codex-harness/resource-broker/v1";
fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn random_key() -> io::Result<String> {
    let mut random = [0u8; 32];
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            random.as_mut_ptr(),
            random.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        return Err(io::Error::other(format!(
            "broker key generation failed: 0x{:08x}",
            status as u32
        )));
    }
    Ok(random.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    schema: String,
    pub pid: u32,
    pub creation_time: u64,
    program: PathBuf,
    program_sha256: String,
    pub port: u16,
    token: String,
    pub source: String,
}
impl fmt::Debug for Endpoint {
    fn fmt(&self, format: &mut fmt::Formatter<'_>) -> fmt::Result {
        format
            .debug_struct("Endpoint")
            .field("pid", &self.pid)
            .field("creation_time", &self.creation_time)
            .field("port", &self.port)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}
impl Endpoint {
    fn current(port: u16, source: &str) -> io::Result<Self> {
        if port == 0 || !digest(source) {
            return Err(invalid("invalid broker endpoint configuration"));
        }
        let program = std::env::current_exe()?.canonicalize()?;
        let _guard = ReadGuard::open(&program)?;
        let owner = ServiceProcess::observe(std::process::id(), &program, 0, &current_user()?)?;
        let token = random_key()?;
        Ok(Self {
            schema: SCHEMA.into(),
            pid: owner.identity().pid,
            creation_time: owner.identity().creation_time,
            program_sha256: build_identity::hash_file(&program)?,
            program,
            port,
            token,
            source: source.into(),
        })
    }
    fn parse(bytes: &[u8]) -> io::Result<Self> {
        let value: Self = serde_json::from_value(
            strict_json(bytes)
                .map_err(|_| invalid("invalid broker endpoint JSON; preserving state"))?,
        )
        .map_err(|_| invalid("invalid broker endpoint fields; preserving state"))?;
        if value.schema != SCHEMA
            || value.pid == 0
            || value.creation_time == 0
            || value.port == 0
            || !value.program.is_absolute()
            || !digest(&value.program_sha256)
            || !digest(&value.token)
            || !digest(&value.source)
        {
            return Err(invalid(
                "unsupported broker endpoint identity; preserving state",
            ));
        }
        validate_local_path(&value.program)
            .map_err(|_| invalid("broker executable must be an unambiguous local path"))?;
        Ok(value)
    }
    pub fn identity(&self) -> ProcessIdentity {
        ProcessIdentity {
            pid: self.pid,
            creation_time: self.creation_time,
        }
    }
    /// The bearer stays out of Debug/status output; only the transport needs it.
    pub fn token(&self) -> &str {
        &self.token
    }
    fn owner(&self) -> io::Result<Option<ServiceProcess>> {
        let Some(owner) =
            ServiceProcess::inspect(self.identity(), &self.program, &current_user()?)?
        else {
            return Ok(None);
        };
        let _guard = ReadGuard::open(&self.program)?;
        if build_identity::hash_file(&self.program)? != self.program_sha256 {
            return Err(invalid(
                "broker owner executable changed; preserving process",
            ));
        }
        Ok(Some(owner))
    }
}

pub enum Observation {
    Absent,
    Stale,
    Ready {
        endpoint: Endpoint,
        owner: ServiceProcess,
    },
}
pub fn observe(root: &BrokerRoot) -> io::Result<Observation> {
    let bytes = match root.read_private(FILE, 4096) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Observation::Absent),
        Err(error) => return Err(error),
    };
    let endpoint = Endpoint::parse(&bytes)?;
    match endpoint.owner()? {
        Some(owner) => Ok(Observation::Ready { endpoint, owner }),
        None => Ok(Observation::Stale),
    }
}

/// An instance claim must stay alive until all service workers have stopped.
/// Dropping without close leaves a receipt for a later exact stale-owner check.
pub struct Instance<'a> {
    root: &'a BrokerRoot,
    _lease: Lease,
    published: Option<Vec<u8>>,
}
impl<'a> Instance<'a> {
    pub fn claim(root: &'a BrokerRoot) -> io::Result<Self> {
        let lease = root.try_instance()?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "broker instance owner is active; preserving unpublished or ready owner",
            )
        })?;
        match root.read_private(FILE, 4096) {
            Ok(bytes) => {
                let endpoint = Endpoint::parse(&bytes)?;
                if endpoint.owner()?.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "broker receipt still has a live owner; preserving process",
                    ));
                }
                // No live instance lease, and the exact PID/creation receipt is
                // stale. Delete only these inspected bytes through the native guard.
                FileGuard::open_regular(&root.path().join(FILE), &bytes)?.remove()?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
        Ok(Self {
            root,
            _lease: lease,
            published: None,
        })
    }
    pub fn publish(&mut self, port: u16, source: &str) -> io::Result<Endpoint> {
        if self.published.is_some() {
            return Err(invalid("broker endpoint was already published"));
        }
        self.root.ensure_private()?;
        let endpoint = Endpoint::current(port, source)?;
        let bytes = serde_json::to_vec(&endpoint)
            .map_err(|_| invalid("broker endpoint encoding failed"))?;
        if bytes.len() > 4096 {
            return Err(invalid("broker endpoint exceeds its bound"));
        }
        StagedFile::create(&self.root.path().join(FILE), &bytes)?.commit()?;
        self.published = Some(bytes);
        Ok(endpoint)
    }
    pub fn close(self) -> io::Result<()> {
        if let Some(bytes) = &self.published {
            FileGuard::open_regular(&self.root.path().join(FILE), bytes)?.remove()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, net::TcpListener};
    const SOURCE: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    #[test]
    fn receipt_binds_current_process_and_secret_is_not_debug_output() {
        let prepared = BrokerRoot::prepare().unwrap();
        let root = prepared.root();
        assert!(matches!(observe(root).unwrap(), Observation::Absent));
        let mut instance = Instance::claim(root).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = instance
            .publish(listener.local_addr().unwrap().port(), SOURCE)
            .unwrap();
        assert!(!format!("{endpoint:?}").contains(endpoint.token()));
        match observe(root).unwrap() {
            Observation::Ready {
                endpoint: read,
                owner,
            } => {
                assert_eq!(owner.identity(), endpoint.identity());
                assert_eq!(read.token(), endpoint.token());
            }
            _ => panic!("published current process not observed"),
        }
        assert_eq!(
            Instance::claim(root).err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        instance.close().unwrap();
        assert!(matches!(observe(root).unwrap(), Observation::Absent));
        assert!(root.path().join("instance.lock").exists());
    }
    #[test]
    fn unpublished_owner_and_foreign_endpoint_are_never_overwritten() {
        let prepared = BrokerRoot::prepare().unwrap();
        let root = prepared.root();
        let instance = Instance::claim(root).unwrap();
        assert_eq!(
            Instance::claim(root).err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        instance.close().unwrap();
        fs::write(root.path().join(FILE), b"foreign private sentinel").unwrap();
        assert!(Instance::claim(root).is_err());
        assert_eq!(
            fs::read(root.path().join(FILE)).unwrap(),
            b"foreign private sentinel"
        );
    }
    #[test]
    fn changed_receipt_is_preserved_and_exact_stale_identity_can_be_reclaimed() {
        let prepared = BrokerRoot::prepare().unwrap();
        let root = prepared.root();
        let mut instance = Instance::claim(root).unwrap();
        let endpoint = instance.publish(12345, SOURCE).unwrap();
        let path = root.path().join(FILE);
        fs::write(&path, b"foreign edit").unwrap();
        assert!(instance.close().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"foreign edit");
        let mut stale = endpoint;
        stale.creation_time -= 1; // Exact mismatch against a still-live PID, not PID kill authority.
        fs::write(&path, serde_json::to_vec(&stale).unwrap()).unwrap();
        assert!(matches!(observe(root).unwrap(), Observation::Stale));
        Instance::claim(root).unwrap().close().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn endpoint_paths_and_duplicate_fields_fail_before_process_or_network_lookup() {
        let mut endpoint = Endpoint::current(12345, SOURCE).unwrap();
        for path in [
            r"\\example.invalid\share\foreign.exe",
            r"\\.\PIPE\foreign",
            r"C:\owned\..\foreign.exe",
        ] {
            endpoint.program = path.into();
            assert!(Endpoint::parse(&serde_json::to_vec(&endpoint).unwrap()).is_err());
        }
        assert!(Endpoint::parse(br#"{"schema":"private-sentinel","schema":"duplicate"}"#).is_err());
    }
}
