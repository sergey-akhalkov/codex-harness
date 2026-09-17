//! Long-lived child fixture for background-spawn handle checks.
//! Stays alive without touching stdio so tests can observe which stdio
//! handles a background spawn leaves open.
fn main() {
    std::thread::sleep(std::time::Duration::from_secs(30));
}
