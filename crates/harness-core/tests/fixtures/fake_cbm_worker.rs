//! Native process double for the audited CBM worker transport, never registered.
use std::{
    env, fs,
    io::{self, Write},
    process::Command,
    thread,
    time::Duration,
};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--late-writer") {
        thread::sleep(Duration::from_millis(500));
        fs::write(&args[1], b"late-child-wrote").unwrap();
        thread::sleep(Duration::from_secs(60));
        return;
    }
    let cli = args.get(1).is_some_and(|value| value == "--json");
    let (request, response_path) = if cli {
        assert_eq!(args.len(), 5);
        assert_eq!(
            &args[..4],
            ["cli", "--json", "list_projects", "--args-file"]
        );
        (
            fs::read_to_string(&args[4]).unwrap(),
            env::current_dir().unwrap().join("response.json"),
        )
    } else {
        assert_eq!(args.len(), 10);
        assert_eq!(
            &args[..5],
            [
                "cli",
                "--index-worker",
                "--index-worker-build",
                "b4b403b1d7c4def3785f148b93f345ce8427858f4f5489ce28580c4387a336a6",
                "index_repository"
            ]
        );
        assert_eq!(args[6], "--response-out");
        assert_eq!(args[8], "--index-worker-memory-budget-bytes");
        assert_eq!(args[9], "1073741824");
        (args[5].clone(), std::path::PathBuf::from(&args[7]))
    };
    for (key, value) in [
        ("CBM_WORKERS", "2"),
        ("CBM_MEM_BUDGET_MB", "1024"),
        ("CBM_RETAIN_TOTAL_MB", "8"),
        ("CBM_RETAIN_PER_FILE_MB", "1"),
    ] {
        assert_eq!(env::var(key).unwrap(), value);
    }
    assert!(env::var_os("OPENAI_API_KEY").is_none());
    assert!(env::var_os("PATH").is_none());
    if cli {
        assert!(
            request.contains("\"payload\":\"Привет 日本\""),
            "UTF-8 argument file changed"
        );
    }
    if request.contains("\"mode\":\"hang\"") {
        thread::sleep(Duration::from_secs(60));
        return;
    }
    if request.contains("\"mode\":\"flood\"") {
        let block = [b'X'; 65536];
        loop {
            if io::stderr().write_all(&block).is_err() {
                return;
            }
        }
    }
    if request.contains("\"mode\":\"stdout-flood\"") {
        let block = [b'X'; 65536];
        loop {
            if io::stdout().write_all(&block).is_err() {
                return;
            }
        }
    }
    if request.contains("\"mode\":\"exit\"") {
        std::process::exit(37);
    }
    if request.contains("\"mode\":\"cli-empty-failure\"") {
        eprintln!("private upstream startup failure");
        std::process::exit(1);
    }
    if request.contains("\"mode\":\"response-flood\"") {
        fs::File::create(&response_path)
            .unwrap()
            .set_len(64 * 1024 * 1024)
            .unwrap();
        return;
    }
    if request.contains("\"mode\":\"diagnostic-success\"")
        || request.contains("\"mode\":\"diagnostic-failure\"")
    {
        fs::create_dir(response_path.parent().unwrap().join("process.json")).unwrap();
        if request.contains("\"mode\":\"diagnostic-failure\"") {
            std::process::exit(37);
        }
    }
    if request.contains("\"mode\":\"late\"") {
        let child = Command::new(env::current_exe().unwrap())
            .arg("--late-writer")
            .arg(&response_path)
            .spawn()
            .unwrap();
        // Deliberately retain a child after root exit; the parent harness owns it.
        drop(child);
    }
    let response = if request.contains("\"mode\":\"duplicate\"") {
        "{\"content\":[],\"isError\":true,\"isError\":false}"
    } else if request.contains("\"mode\":\"malformed\"") {
        "{\"content\":false}"
    } else if request.contains("\"mode\":\"tool-error\"") {
        "{\"content\":[{\"type\":\"text\",\"text\":\"owned tool error\"}],\"isError\":true}"
    } else {
        "{\"content\":[{\"type\":\"text\",\"text\":\"α 日本 😀\"}]}"
    };
    if cli {
        println!("{response}");
        if request.contains("\"mode\":\"tool-error\"")
            || request.contains("\"mode\":\"exit-mismatch\"")
        {
            std::process::exit(1);
        }
    } else {
        fs::write(&response_path, response).unwrap();
    }
}
