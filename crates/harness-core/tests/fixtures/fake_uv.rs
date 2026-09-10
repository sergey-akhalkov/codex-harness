//! Native uv double for bounded venv staging. Never registered as uv.
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

fn fail(code: i32) -> ! {
    std::process::exit(code);
}

fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn mode_path() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("fake-uv-mode.txt")))
        .unwrap_or_else(|| PathBuf::from("fake-uv-mode.txt"))
}

fn arg_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let mut items = args.iter();
    while let Some(item) = items.next() {
        if item == name {
            return items.next().map(String::as_str);
        }
        if let Some(value) = item.strip_prefix(&format!("{name}=")) {
            return Some(value);
        }
    }
    None
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|item| item == name)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--version") {
        println!("uv 0.11.32 (double)");
        return;
    }
    if args.first().map(String::as_str) != Some("venv") {
        fail(2);
    }
    if env::var_os("PATH").is_some() {
        fail(3);
    }
    let mode = fs::read_to_string(mode_path()).unwrap_or_default();
    match mode.trim() {
        "timeout" => loop {
            thread::sleep(Duration::from_secs(30));
        },
        "flood" => loop {
            let _ = writeln!(io::stderr(), "{}", "x".repeat(8192));
        },
        "fail" => {
            let _ = writeln!(io::stderr(), "uv double refused venv");
            fail(7);
        }
        "install-package" => {
            let _ = writeln!(io::stderr(), "Would install packages");
            fail(9);
        }
        _ => {}
    }

    let required = [
        "--no-project",
        "--no-python-downloads",
        "--offline",
        "--no-config",
        "--no-progress",
    ];
    if required.iter().any(|flag| !has_flag(&args, flag))
        || has_flag(&args, "--seed")
        || has_flag(&args, "--relocatable")
        || has_flag(&args, "--allow-existing")
        || has_flag(&args, "--clear")
        || has_flag(&args, "--force")
    {
        fail(4);
    }

    let python = arg_value(&args, "--python").unwrap_or_else(|| fail(5));
    let cache = arg_value(&args, "--cache-dir").unwrap_or_else(|| fail(5));
    let directory = arg_value(&args, "--directory").unwrap_or_else(|| fail(5));
    let target = args.get(1).map(String::as_str).unwrap_or_else(|| fail(5));
    if !Path::new(python).is_absolute()
        || !Path::new(cache).is_absolute()
        || !Path::new(directory).is_absolute()
        || !Path::new(target).is_absolute()
    {
        fail(6);
    }
    if Path::new(target).exists() {
        fail(8);
    }

    let cwd = env::current_dir().unwrap_or_else(|_| fail(10));
    if cwd != Path::new(directory) {
        fail(11);
    }
    for name in [
        "UV_CACHE_DIR",
        "UV_PYTHON_INSTALL_DIR",
        "UV_TOOL_DIR",
        "UV_TOOL_BIN_DIR",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
        "TMP",
    ] {
        let value = env::var_os(name).unwrap_or_else(|| fail(12));
        if !Path::new(&value).is_absolute() {
            fail(12);
        }
    }
    if env::var("UV_NO_CONFIG").ok().as_deref() != Some("1")
        || env::var("UV_OFFLINE").ok().as_deref() != Some("1")
        || env::var("UV_PYTHON_DOWNLOADS").ok().as_deref() != Some("never")
    {
        fail(13);
    }

    let python_path = PathBuf::from(python);
    let home = python_path.parent().unwrap_or_else(|| fail(14));
    let cache_path = PathBuf::from(cache);
    let marker = cache_path.join("interpreter-v4").join("observed.msgpack");
    if write_file(&marker, b"uv-double-cache").is_err() {
        fail(15);
    }
    let venv = PathBuf::from(target);
    let pyvenv = format!(
        "home = {}\nimplementation = CPython\nuv = 0.11.32\nversion_info = 3.13.14\ninclude-system-site-packages = false\n",
        home.display()
    );
    if write_file(&venv.join("pyvenv.cfg"), pyvenv.as_bytes()).is_err()
        || write_file(&venv.join("Scripts/python.exe"), b"uv-double-python-launcher").is_err()
        || write_file(&venv.join("Scripts/pythonw.exe"), b"uv-double-pythonw-launcher").is_err()
        || fs::create_dir_all(venv.join("Lib/site-packages")).is_err()
        || write_file(&venv.join("Lib/site-packages/_virtualenv.pth"), b"").is_err()
    {
        fail(16);
    }
    let _ = writeln!(
        io::stderr(),
        "Using CPython 3.13.14 interpreter at: {}",
        python_path.display()
    );
    let _ = writeln!(
        io::stderr(),
        "Creating virtual environment at: {}",
        venv.display()
    );
}
