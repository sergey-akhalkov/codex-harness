//! Process-boundary double for dependency validation. Never registered as MCP.
#[cfg(windows)]
mod windows {
    use serde_json::{Value, json};
    use std::{
        fs,
        io::{self, BufRead, Write},
        os::windows::ffi::OsStrExt,
        path::Path,
        process::{Command, Stdio},
        time::Duration,
    };
    use windows_sys::Win32::{
        Security::*,
        System::{JobObjects::*, Memory::*, Threading::GetCurrentProcess},
    };

    fn emit(value: Value, fragmented: bool) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        let mut output = io::stdout().lock();
        if fragmented {
            for byte in bytes {
                output.write_all(&[byte])?;
                output.flush()?;
            }
        } else {
            output.write_all(&bytes)?;
            output.flush()?;
        }
        Ok(())
    }
    fn tool(name: &str, properties: Value, required: Value) -> Value {
        json!({"name":name,"description":"SECRET_FOREIGN_TOKEN","inputSchema":{"type":"object","properties":properties,"required":required}})
    }
    fn catalogue(cbm: bool) -> Vec<Value> {
        if cbm {
            return vec![
                tool(
                    "index_repository",
                    json!({"repo_path":{"type":"string"},"mode":{"type":"string","enum":["full","moderate","fast","cross-repo-intelligence"],"default":"full"}}),
                    json!(["repo_path"]),
                ),
                tool("search_graph", json!({}), json!([])),
                tool("list_projects", json!({}), json!([])),
                tool(
                    "get_graph_schema",
                    json!({"project":{"type":"string"}}),
                    json!(["project"]),
                ),
                tool(
                    "query_graph",
                    json!({"project":{"type":"string"},"query":{"type":"string"}}),
                    json!(["query", "project"]),
                ),
            ];
        }
        // Independent test catalogue from upstream v0.2.2 schemas.rs.
        let names = "desktop_screen_size desktop_screenshot desktop_windows_list desktop_window_activate desktop_window_screenshot desktop_window_move desktop_window_resize desktop_window_info desktop_vision desktop_perceive desktop_mouse desktop_mouse_drag desktop_input desktop_clipboard_clean desktop_clipboard_write browser_navigate browser_snapshot browser_exec browser_click browser_type browser_press browser_scroll browser_extract browser_screenshot browser_close browser_evaluate browser_back browser_forward browser_wait_for browser_cookies_get browser_cookies_set browser_import_cookies browser_upload browser_drag_files browser_list_downloads browser_new_tab browser_list_tabs browser_switch_tab";
        names.split_whitespace().map(|name| {
            let mut value = tool(name, json!({}), json!([]));
            if ["browser_click","browser_type","browser_drag_files"].contains(&name) {
                let mut properties = json!({"selector":{"type":"string","minLength":1},"ref":{"type":"string","minLength":1}});
                let required = match name {
                    "browser_type" => { properties["text"] = json!({"type":"string"}); json!(["text"]) }
                    "browser_drag_files" => { properties["file_paths"] = json!({"type":"array","items":{"type":"string"},"minItems":1}); json!(["file_paths"]) }
                    _ => json!([]),
                };
                value["inputSchema"] = json!({"type":"object","properties":properties,"required":required,"anyOf":[{"required":["selector"]},{"required":["ref"]}]});
            }
            value
        }).collect()
    }
    fn tool_result(value: Value, text: bool) -> Value {
        if text {
            json!({"content":[{"type":"text","text":value.to_string()}]})
        } else {
            json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false})
        }
    }
    fn owner_acl(root: &Path, protected: bool) -> bool {
        let wide: Vec<_> = root.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut buffer = [0u64; 1024];
        let mut needed = 0;
        unsafe {
            if GetFileSecurityW(
                wide.as_ptr(),
                DACL_SECURITY_INFORMATION,
                buffer.as_mut_ptr().cast(),
                size_of_val(&buffer) as u32,
                &mut needed,
            ) == 0
            {
                return false;
            }
            let (mut acl, mut present, mut defaulted) = (std::ptr::null_mut(), 0, 0);
            let (mut control, mut revision) = (0, 0);
            let sd = buffer.as_mut_ptr().cast();
            GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted) != 0
                && present != 0
                && !acl.is_null()
                && (*acl).AceCount == 1
                && GetSecurityDescriptorControl(sd, &mut control, &mut revision) != 0
                && (!protected || control & SE_DACL_PROTECTED != 0)
        }
    }
    fn linger() {
        std::thread::sleep(Duration::from_secs(45));
    }

    pub fn run() -> io::Result<()> {
        if std::env::args().nth(1).as_deref() == Some("--probe") {
            let digest = std::env::args()
                .nth(2)
                .ok_or_else(|| io::Error::other("fixture digest missing"))?;
            let summary = harness_core::dependency_mcp_probe::probe(
                &std::env::current_exe()?,
                harness_core::dependency_mcp_probe::ProbeKind::Nuphus,
                &digest,
            )?;
            println!("{summary}");
            return Ok(());
        }
        if std::env::args().nth(1).as_deref() == Some("--linger") {
            linger();
            return Ok(());
        }
        let executable = std::env::current_exe()?;
        let config: Value = serde_json::from_slice(&fs::read(executable.with_extension("json"))?)?;
        let mode = config["mode"].as_str().unwrap_or("valid");
        let cbm = config["kind"] == "cbm";
        let root = std::env::current_dir()?;
        let mut member = 0;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        let mut cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
        let job = unsafe {
            IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut member) != 0
                && member != 0
                && QueryInformationJobObject(
                    std::ptr::null_mut(),
                    JobObjectExtendedLimitInformation,
                    (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of_val(&limits) as u32,
                    std::ptr::null_mut(),
                ) != 0
                && QueryInformationJobObject(
                    std::ptr::null_mut(),
                    JobObjectCpuRateControlInformation,
                    (&mut cpu as *mut JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
                    size_of_val(&cpu) as u32,
                    std::ptr::null_mut(),
                ) != 0
        };
        let isolated = [
            "HOME",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "TEMP",
            "TMP",
            "XDG_CACHE_HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "CODEX_HOME",
            "CBM_CACHE_DIR",
            "CBM_RUNTIME_DIR",
            "NUPHUS_MODELS_DIR",
        ]
        .iter()
        .all(|key| std::env::var_os(key).is_some_and(|v| Path::new(&v).starts_with(&root)));
        let mut receipt = json!({"pid":std::process::id(),"cwd":root,"in_job_at_entry":job,
            "memory_limit":limits.JobMemoryLimit,"cpu_rate":unsafe{cpu.Anonymous.CpuRate},"cpu_hard_cap":cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP != 0,
            "no_breakaway":limits.BasicLimitInformation.LimitFlags & (JOB_OBJECT_LIMIT_BREAKAWAY_OK|JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK) == 0,
            "kill_on_close":limits.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE != 0,
            "private_directories":isolated,"owner_acl":owner_acl(&root,true),"child_acl":owner_acl(&root.join("ipc"),false),"no_models":std::env::var("NUPHUS_MCP_NO_MODEL_DOWNLOAD").ok().as_deref()==Some("1"),
            "ambient_absent":(["PATH","OPENAI_API_KEY","HARNESS_MCP_SECRET","NUPHUS_MCP_BROWSER_CDP_URL","HTTP_PROXY"].iter().all(|k| std::env::var_os(k).is_none())),"calls":[]});
        let mut descendant = if ["valid-tree", "cancel", "deadline", "root-exit"].contains(&mode) {
            let child = Command::new(&executable)
                .arg("--linger")
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()?;
            receipt["child_pid"] = json!(child.id());
            Some(child)
        } else {
            None
        };
        let receipt_path = executable.with_extension("receipt.json");
        fs::write(&receipt_path, serde_json::to_vec(&receipt)?)?;
        match mode {
            "cancel" | "deadline" => {
                linger();
                return Ok(());
            }
            "memory" => {
                let memory = unsafe {
                    VirtualAlloc(
                        std::ptr::null(),
                        768 * 1024 * 1024,
                        MEM_COMMIT | MEM_RESERVE,
                        PAGE_READWRITE,
                    )
                };
                receipt["allocation_denied"] = json!(memory.is_null());
                fs::write(&receipt_path, serde_json::to_vec(&receipt)?)?;
                if !memory.is_null() {
                    unsafe {
                        VirtualFree(memory, 0, MEM_RELEASE);
                    }
                }
                linger();
                return Ok(());
            }
            "stderr-flood" => {
                io::stderr().write_all(&vec![b's'; 512 * 1024])?;
                linger();
                return Ok(());
            }
            "stdout-flood" => {
                for _ in 0..6 {
                    emit(
                        json!({"jsonrpc":"2.0","method":"notifications/message","params":{"data":"x".repeat(900*1024)}}),
                        false,
                    )?;
                }
                linger();
                return Ok(());
            }
            "oversized" => {
                io::stdout().write_all(&vec![b'x'; 1024 * 1024 + 1])?;
                linger();
                return Ok(());
            }
            "malformed" => {
                let _ = io::stdin().lock().lines().next();
                println!("SECRET_FOREIGN_TOKEN invalid output");
                return Ok(());
            }
            "invalid-utf8" => {
                let _ = io::stdin().lock().lines().next();
                io::stdout().write_all(&[0xff, b'\n'])?;
                return Ok(());
            }
            "incomplete" => {
                let _ = io::stdin().lock().lines().next();
                print!("{{\"jsonrpc\":\"2.0\"");
                io::stdout().flush()?;
                return Ok(());
            }
            "early-eof" => {
                let _ = io::stdin().lock().lines().next();
                return Ok(());
            }
            "early-nonzero" => {
                let _ = io::stdin().lock().lines().next();
                std::process::exit(19);
            }
            _ => {}
        }
        let mut initialized = false;
        let mut declared = false;
        let mut pages = 0;
        let mut calls = 0;
        for line in io::stdin().lock().lines() {
            let request: Value = serde_json::from_str(&line?)?;
            assert_eq!(request["jsonrpc"], "2.0");
            let method = request["method"].as_str().unwrap();
            receipt["calls"].as_array_mut().unwrap().push(json!(method));
            fs::write(&receipt_path, serde_json::to_vec(&receipt)?)?;
            match mode {
                "wrong-id" => {
                    emit(json!({"jsonrpc":"2.0","id":42,"result":{}}), false)?;
                    return Ok(());
                }
                "string-id" => {
                    emit(json!({"jsonrpc":"2.0","id":"1","result":{}}), false)?;
                    return Ok(());
                }
                "error" => {
                    emit(
                        json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-1,"message":"SECRET_FOREIGN_TOKEN"}}),
                        false,
                    )?;
                    return Ok(());
                }
                "duplicate-key" => {
                    println!("{{\"jsonrpc\":\"2.0\",\"id\":9,\"id\":1,\"result\":{{}}}}");
                    return Ok(());
                }
                "server-request" => {
                    emit(
                        json!({"jsonrpc":"2.0","id":1,"method":"roots/list","params":{}}),
                        false,
                    )?;
                    return Ok(());
                }
                "notifications" => {
                    for _ in 0..129 {
                        emit(
                            json!({"jsonrpc":"2.0","method":"notifications/message","params":{"data":"SECRET_FOREIGN_TOKEN"}}),
                            false,
                        )?;
                    }
                    return Ok(());
                }
                _ => {}
            }
            let mut result = match method {
                "initialize" => {
                    assert!(!initialized);
                    assert_eq!(request["params"]["protocolVersion"], "2024-11-05");
                    assert_eq!(request["params"]["capabilities"], json!({}));
                    initialized = true;
                    json!({"protocolVersion":if mode=="protocol" {"2099-01-01"} else {"2024-11-05"},"capabilities":{"tools":{}},"serverInfo":{"name":if cbm {"codebase-memory-mcp"} else {"nuphus-mcp"},"version":"0.2.2"},"instructions":"SECRET_FOREIGN_TOKEN"})
                }
                "notifications/initialized" => {
                    assert!(initialized && !declared);
                    assert!(request.get("id").is_none());
                    declared = true;
                    continue;
                }
                "tools/list" => {
                    assert!(declared);
                    let mut tools = catalogue(cbm);
                    match mode {
                        "missing-tool" => {
                            tools.pop();
                        }
                        "renamed-tool" => {
                            tools[0]["name"] = json!("unknown");
                        }
                        "duplicate-tool" => tools.push(tools[0].clone()),
                        "bad-schema" => tools[0]["inputSchema"]["type"] = json!("string"),
                        "bad-alias" => {
                            tools
                                .iter_mut()
                                .find(|t| t["name"] == "browser_click")
                                .unwrap()["inputSchema"]["anyOf"][1]["required"] = json!(["other"]);
                        }
                        "missing-mode" => {
                            tools[0]["inputSchema"]["properties"]
                                .as_object_mut()
                                .unwrap()
                                .remove("mode");
                        }
                        "fixed-schema" => {
                            for tool in &mut tools {
                                if let Some(branches) = tool["inputSchema"]["anyOf"].as_array_mut()
                                {
                                    for branch in branches {
                                        branch["type"] = json!("object");
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    pages += 1;
                    if mode == "cursor-loop" {
                        json!({"tools":[],"nextCursor":"same"})
                    } else if mode == "paged" {
                        let split = tools.len().div_ceil(2);
                        if pages == 1 {
                            assert!(request["params"].get("cursor").is_none());
                            json!({"tools":&tools[..split],"nextCursor":"next"})
                        } else {
                            assert_eq!(request["params"]["cursor"], "next");
                            json!({"tools":&tools[split..]})
                        }
                    } else {
                        json!({"tools":tools})
                    }
                }
                "tools/call" => {
                    assert!(cbm && declared, "Nuphus must never receive tools/call");
                    let names = [
                        "index_repository",
                        "list_projects",
                        "get_graph_schema",
                        "query_graph",
                    ];
                    assert_eq!(request["params"]["name"], names[calls]);
                    calls += 1;
                    match request["params"]["name"].as_str().unwrap() {
                        "index_repository" => {
                            let path = request["params"]["arguments"]["repo_path"]
                                .as_str()
                                .unwrap();
                            assert_eq!(Path::new(path), root.join("fixture"));
                            assert!(
                                fs::read_to_string(Path::new(path).join("probe.rs"))?
                                    .contains("fn inventory_probe(")
                            );
                            assert_eq!(request["params"]["arguments"]["mode"], "fast");
                            if mode.starts_with("hold-state") {
                                fs::write(root.join("held-by-test"), b"hold")?;
                                let until = std::time::Instant::now() + Duration::from_secs(5);
                                while !root.join("hold-ready").exists() {
                                    assert!(
                                        std::time::Instant::now() < until,
                                        "test did not acquire its hold"
                                    );
                                    std::thread::sleep(Duration::from_millis(10));
                                }
                            }
                            if mode == "index-error" {
                                json!({"isError":true,"content":[{"type":"text","text":"SECRET_FOREIGN_TOKEN"}]})
                            } else {
                                tool_result(json!({"indexed":true}), false)
                            }
                        }
                        "list_projects" => {
                            let mut value = json!({"projects":[{"name":"fixture","root_path":root.join("fixture")}],"total":1,"has_more":false});
                            if mode == "wrong-root" {
                                value["projects"][0]["root_path"] = json!(root);
                            }
                            if mode == "multiple-projects" {
                                value["total"] = json!(2);
                            }
                            tool_result(value, mode == "text-payload")
                        }
                        "get_graph_schema" => tool_result(json!({"labels":["Function"]}), false),
                        "query_graph" => {
                            assert_eq!(request["params"]["arguments"]["format"], "json");
                            assert_eq!(request["params"]["arguments"]["project"], "fixture");
                            let mut value = json!({"columns":["n.name"],"rows":[["inventory_probe"]],"total":1});
                            if matches!(mode, "query-spoof" | "hold-state-query-spoof") {
                                value["rows"] = json!([]);
                                value["warning"] = json!("inventory_probe");
                            }
                            tool_result(value, mode == "text-payload")
                        }
                        _ => unreachable!(),
                    }
                }
                _ => panic!("unexpected protocol method"),
            };
            if mode == "missing-capability" {
                result["capabilities"] = json!({});
            }
            emit(
                json!({"jsonrpc":"2.0","method":"notifications/message","params":{"data":"SECRET_FOREIGN_TOKEN"}}),
                false,
            )?;
            emit(
                json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
                mode == "fragmented",
            )?;
            let final_reply = (!cbm && method == "tools/list") || (cbm && calls == 4);
            if final_reply {
                match mode {
                    "trailing-id" => emit(
                        json!({"jsonrpc":"2.0","id":request["id"],"result":{}}),
                        false,
                    )?,
                    "trailing-partial" => {
                        print!("partial SECRET_FOREIGN_TOKEN");
                        io::stdout().flush()?;
                    }
                    "root-exit" => return Ok(()),
                    _ => {}
                }
            }
        }
        if mode == "late-nonzero" {
            std::process::exit(23);
        }
        if cbm {
            assert_eq!(
                calls,
                if config["catalogue_only"] == true {
                    0
                } else {
                    4
                }
            );
        }
        if let Some(child) = descendant.as_mut() {
            assert!(child.try_wait()?.is_none());
        }
        receipt["stdin_eof"] = json!(true);
        fs::write(receipt_path, serde_json::to_vec(&receipt)?)?;
        Ok(())
    }
}

fn main() {
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() == Some("--stdio-session") {
        use harness_core::{
            mcp_session::Session,
            mcp_stdio,
            process::{Cancellation, Deadline},
        };
        use serde_json::json;
        use std::time::Duration;
        let result = (|| -> std::io::Result<()> {
            let session = Session::new(
                "harness-stdio-fixture",
                vec![
                    json!({"name":"echo","inputSchema":{"type":"object","properties":{}}}),
                    json!({"name":"wait","inputSchema":{"type":"object","properties":{}}}),
                    json!({"name":"wait-panic","inputSchema":{"type":"object","properties":{}}}),
                    json!({"name":"wait-error","inputSchema":{"type":"object","properties":{}}}),
                    json!({"name":"flood","inputSchema":{"type":"object","properties":{}}}),
                ],
            )?;
            let (input, output) = mcp_stdio::standard_files()?;
            mcp_stdio::serve_fallible(
                session,
                input,
                output,
                &Cancellation::default(),
                Deadline::after(Duration::from_secs(30))?,
                |operation| {
                    if operation.arguments["mark_start"] == true {
                        std::fs::write("queued-started", b"started").unwrap();
                    }
                    if matches!(
                        operation.name.as_str(),
                        "wait" | "wait-panic" | "wait-error"
                    ) {
                        while !operation.cancellation.is_cancelled()
                            && !operation.deadline.expired()
                        {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        // Observable cleanup delay: the next task must not begin yet.
                        std::thread::sleep(Duration::from_millis(80));
                        assert_ne!(
                            operation.name, "wait-panic",
                            "owned cancellation panic fixture"
                        );
                        if operation.name == "wait-error" {
                            return Err(std::io::Error::other("owned backend cleanup failure"));
                        }
                    }
                    if operation.name == "flood" {
                        return Ok(
                            json!({"content":[{"type":"text","text":"x".repeat(2 * 1024 * 1024)}]}),
                        );
                    }
                    Ok(
                        json!({"content":[{"type":"text","text":operation.arguments.to_string()}],"structuredContent":operation.arguments}),
                    )
                },
            )
        })();
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(91);
        }
        return;
    }
    #[cfg(windows)]
    if windows::run().is_err() {
        std::process::exit(90);
    }
}
