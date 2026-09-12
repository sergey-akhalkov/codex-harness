//! Owned Task Scheduler definitions. Registration does not start a task.
#![cfg(windows)]

use std::{
    ffi::OsString,
    io,
    os::windows::{ffi::OsStringExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
};
use windows::{
    Win32::{
        Foundation::{VARIANT_FALSE, VARIANT_TRUE},
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            TaskScheduler::{
                IExecAction, ILogonTrigger, IRegisteredTask, ITaskDefinition, ITaskFolder,
                ITaskService, TASK_ACTION_EXEC, TASK_CREATE, TASK_IGNORE_REGISTRATION_TRIGGERS,
                TASK_INSTANCES_IGNORE_NEW, TASK_LOGON_INTERACTIVE_TOKEN, TASK_RUNLEVEL_HIGHEST,
                TASK_RUNLEVEL_LUA, TASK_STATE_RUNNING, TASK_TRIGGER_LOGON, TASK_UPDATE,
                TaskScheduler,
            },
            Variant::VARIANT,
        },
    },
    core::{BSTR, Interface},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, LocalFree},
    Security::{
        Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY,
        TOKEN_USER, TokenElevation, TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

fn conflict(message: &str) -> io::Error {
    io::Error::other(message)
}

fn com(error: windows::core::Error) -> io::Error {
    conflict(&format!(
        "Task Scheduler call failed: 0x{:08x}",
        error.code().0 as u32
    ))
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn checked(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn invalid_name(name: &str) -> bool {
    name.is_empty()
        || name.contains(char::from(92))
        || name.contains('/')
        || name.contains(char::from(0))
}

fn current_sid() -> io::Result<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    checked(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) })?;
    if token.is_null() {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut needed = 0;
        unsafe {
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        }
        if needed == 0 || needed > 65536 {
            return Err(conflict("task principal identity size limit"));
        }
        let mut buffer = vec![0u8; needed as usize];
        checked(unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        })?;
        let sid = unsafe { (*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        let mut text = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0;
        while unsafe { *text.add(len) } != 0 {
            len += 1;
            if len > 256 {
                unsafe {
                    LocalFree(text.cast());
                }
                return Err(conflict("task principal SID exceeds its bound"));
            }
        }
        let owned = unsafe { OsString::from_wide(std::slice::from_raw_parts(text, len)) };
        unsafe {
            LocalFree(text.cast());
        }
        owned
            .into_string()
            .map_err(|_| conflict("task principal SID is not UTF-8"))
    })();
    unsafe {
        CloseHandle(token);
    }
    result
}

fn elevated() -> io::Result<bool> {
    let mut token: HANDLE = std::ptr::null_mut();
    checked(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) })?;
    if token.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut needed = 0;
    let status = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut needed,
        )
    };
    unsafe {
        CloseHandle(token);
    }
    checked(status)?;
    Ok(elevation.TokenIsElevated != 0)
}

fn connect() -> io::Result<(Apartment, ITaskService, ITaskFolder)> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(com)?
    };
    let apartment = Apartment;
    let service: ITaskService =
        unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(com)? };
    unsafe {
        service
            .Connect(
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
            )
            .map_err(com)?;
    }
    let root = format!("{}", char::from(92));
    let folder = unsafe { service.GetFolder(&BSTR::from(root.as_str())).map_err(com)? };
    Ok((apartment, service, folder))
}

fn xml_text(definition: &ITaskDefinition) -> io::Result<String> {
    let mut xml = BSTR::new();
    unsafe { definition.XmlText(&mut xml).map_err(com)? };
    Ok(xml.to_string())
}

fn get_task(folder: &ITaskFolder, name: &str) -> io::Result<Option<IRegisteredTask>> {
    match unsafe { folder.GetTask(&BSTR::from(name)) } {
        Ok(task) => Ok(Some(task)),
        Err(error) if error.code().0 as u32 == 0x8007_0002 => Ok(None),
        Err(error) => Err(com(error)),
    }
}

pub struct ObservedTask {
    pub xml: String,
    pub running: bool,
}

pub fn observe(name: &str) -> io::Result<Option<ObservedTask>> {
    if invalid_name(name) {
        return Err(conflict("owned task name is invalid"));
    }
    let (_apartment, _service, folder) = connect()?;
    let Some(task) = get_task(&folder, name)? else {
        return Ok(None);
    };
    let xml = unsafe { task.Xml().map_err(com)? }.to_string();
    let running = unsafe { task.State().map_err(com)? } == TASK_STATE_RUNNING;
    Ok(Some(ObservedTask { xml, running }))
}

pub fn equivalent(left: &str, right: &str) -> io::Result<bool> {
    if left == right {
        return Ok(true);
    }
    let (_apartment, service, _folder) = connect()?;
    let parse = |text: &str| -> io::Result<String> {
        let definition = unsafe { service.NewTask(0).map_err(com)? };
        unsafe { definition.SetXmlText(&BSTR::from(text)).map_err(com)? };
        xml_text(&definition)
    };
    match (parse(left), parse(right)) {
        (Ok(left), Ok(right)) => Ok(left == right),
        _ => Ok(false),
    }
}

fn definition(
    service: &ITaskService,
    name: &str,
    home: &Path,
    source: &Path,
    service_state: &Path,
    powershell: &Path,
) -> io::Result<ITaskDefinition> {
    let definition = unsafe { service.NewTask(0).map_err(com)? };
    let info = unsafe { definition.RegistrationInfo().map_err(com)? };
    unsafe {
        info.SetDescription(&BSTR::from(format!(
            "codex-harness subscription routing: {}",
            home.display()
        )))
        .map_err(com)?;
        info.SetURI(&BSTR::from(format!("{}{name}", char::from(92))))
            .map_err(com)?;
    }
    let sid = current_sid()?;
    let principal = unsafe { definition.Principal().map_err(com)? };
    unsafe {
        principal
            .SetUserId(&BSTR::from(sid.as_str()))
            .map_err(com)?;
        principal
            .SetLogonType(TASK_LOGON_INTERACTIVE_TOKEN)
            .map_err(com)?;
        principal
            .SetRunLevel(if elevated()? {
                TASK_RUNLEVEL_HIGHEST
            } else {
                TASK_RUNLEVEL_LUA
            })
            .map_err(com)?;
    }
    let settings = unsafe { definition.Settings().map_err(com)? };
    unsafe {
        settings.SetEnabled(VARIANT_TRUE).map_err(com)?;
        settings.SetHidden(VARIANT_TRUE).map_err(com)?;
        settings
            .SetExecutionTimeLimit(&BSTR::from("PT0S"))
            .map_err(com)?;
        settings.SetRestartCount(3).map_err(com)?;
        settings
            .SetRestartInterval(&BSTR::from("PT1M"))
            .map_err(com)?;
        settings
            .SetMultipleInstances(TASK_INSTANCES_IGNORE_NEW)
            .map_err(com)?;
        settings.SetStartWhenAvailable(VARIANT_TRUE).map_err(com)?;
        settings
            .SetDisallowStartIfOnBatteries(VARIANT_FALSE)
            .map_err(com)?;
        settings
            .SetStopIfGoingOnBatteries(VARIANT_FALSE)
            .map_err(com)?;
    }
    let trigger = unsafe {
        definition
            .Triggers()
            .map_err(com)?
            .Create(TASK_TRIGGER_LOGON)
            .map_err(com)?
    };
    let logon: ILogonTrigger = trigger.cast().map_err(com)?;
    unsafe { logon.SetUserId(&BSTR::from(sid.as_str())).map_err(com)? };
    let action = unsafe {
        definition
            .Actions()
            .map_err(com)?
            .Create(TASK_ACTION_EXEC)
            .map_err(com)?
    };
    let exec: IExecAction = action.cast().map_err(com)?;
    let arguments = format!(
        "-NoLogo -NoProfile -WindowStyle Hidden -File {0:?} -StatePath {1:?}",
        source.join("tools/opencodex-service.ps1").display(),
        service_state.display()
    );
    unsafe {
        exec.SetPath(&BSTR::from(powershell.to_string_lossy().as_ref()))
            .map_err(com)?;
        exec.SetArguments(&BSTR::from(arguments.as_str()))
            .map_err(com)?;
        exec.SetWorkingDirectory(&BSTR::from(source.to_string_lossy().as_ref()))
            .map_err(com)?;
    }
    Ok(definition)
}

pub fn xml(
    name: &str,
    home: &Path,
    source: &Path,
    service_state: &Path,
    powershell: &Path,
) -> io::Result<String> {
    let (_apartment, service, _folder) = connect()?;
    xml_text(&definition(
        &service,
        name,
        home,
        source,
        service_state,
        powershell,
    )?)
}

/// Register or replace an owned task without starting it. A running owned task
/// is refused so the live proxy is preserved.
pub fn register(name: &str, xml: &str, expected: Option<&str>) -> io::Result<String> {
    if invalid_name(name) {
        return Err(conflict("owned task name is invalid"));
    }
    let (_apartment, _service, folder) = connect()?;
    let current = get_task(&folder, name)?;
    match (current.as_ref(), expected) {
        (None, None) => {}
        (Some(task), Some(expected)) => {
            let actual = unsafe { task.Xml().map_err(com)? }.to_string();
            if actual != expected {
                return Err(conflict(
                    "Scheduled task changed; preserving foreign definition.",
                ));
            }
            if unsafe { task.State().map_err(com)? } == TASK_STATE_RUNNING {
                return Err(conflict(
                    "Owned subscription task is running; live proxy was not stopped.",
                ));
            }
        }
        _ => {
            return Err(conflict(
                "Scheduled task changed; preserving foreign definition.",
            ));
        }
    }
    let flags = if current.is_some() {
        TASK_UPDATE.0 | TASK_IGNORE_REGISTRATION_TRIGGERS.0
    } else {
        TASK_CREATE.0 | TASK_IGNORE_REGISTRATION_TRIGGERS.0
    };
    let registered = unsafe {
        folder
            .RegisterTask(
                &BSTR::from(name),
                &BSTR::from(xml),
                flags,
                &VARIANT::default(),
                &VARIANT::default(),
                TASK_LOGON_INTERACTIVE_TOKEN,
                &VARIANT::default(),
            )
            .map_err(com)?
    };
    if unsafe { registered.State().map_err(com)? } == TASK_STATE_RUNNING {
        return Err(conflict(
            "Owned subscription task started during registration; live proxy was not requested.",
        ));
    }
    Ok(unsafe { registered.Xml().map_err(com)? }.to_string())
}

/// Replace an existing owned definition without stopping a running instance.
/// Registration triggers are ignored so the live proxy is not started.
pub fn update_in_place(name: &str, xml: &str, expected: &str) -> io::Result<String> {
    if invalid_name(name) {
        return Err(conflict("owned task name is invalid"));
    }
    let (_apartment, _service, folder) = connect()?;
    let Some(current) = get_task(&folder, name)? else {
        return Err(conflict(
            "Subscription task changed during policy recovery; preserving.",
        ));
    };
    let actual = unsafe { current.Xml().map_err(com)? }.to_string();
    if actual != expected {
        return Err(conflict(
            "Subscription task changed before policy write; preserving.",
        ));
    }
    let was_running = unsafe { current.State().map_err(com)? } == TASK_STATE_RUNNING;
    let registered = unsafe {
        folder
            .RegisterTask(
                &BSTR::from(name),
                &BSTR::from(xml),
                TASK_UPDATE.0 | TASK_IGNORE_REGISTRATION_TRIGGERS.0,
                &VARIANT::default(),
                &VARIANT::default(),
                TASK_LOGON_INTERACTIVE_TOKEN,
                &VARIANT::default(),
            )
            .map_err(com)?
    };
    if !was_running && unsafe { registered.State().map_err(com)? } == TASK_STATE_RUNNING {
        return Err(conflict(
            "Owned subscription task started during registration; live proxy was not requested.",
        ));
    }
    let registered_xml = unsafe { registered.Xml().map_err(com)? }.to_string();
    if !equivalent(&registered_xml, xml)? {
        return Err(conflict(
            "Subscription task differs after policy write; recovery evidence preserved.",
        ));
    }
    Ok(registered_xml)
}

pub fn with_restart_policy(xml: &str, count: i32, interval: &str) -> io::Result<String> {
    let (_apartment, service, _folder) = connect()?;
    let definition = unsafe { service.NewTask(0).map_err(com)? };
    unsafe { definition.SetXmlText(&BSTR::from(xml)).map_err(com)? };
    let settings = unsafe { definition.Settings().map_err(com)? };
    let mut current_count = 0i32;
    let mut current_interval = BSTR::default();
    unsafe {
        settings.RestartCount(&mut current_count).map_err(com)?;
        settings
            .RestartInterval(&mut current_interval)
            .map_err(com)?;
    }
    if current_count == count && current_interval.to_string() == interval {
        return Ok(xml.to_string());
    }
    unsafe {
        settings.SetRestartCount(count).map_err(com)?;
        settings
            .SetRestartInterval(&BSTR::from(interval))
            .map_err(com)?;
    }
    xml_text(&definition)
}

/// Remove an idle owned task. A running owned task is refused so the live
/// proxy is preserved.
pub fn remove(name: &str, expected: Option<&str>) -> io::Result<bool> {
    if invalid_name(name) {
        return Err(conflict("owned task name is invalid"));
    }
    let (_apartment, _service, folder) = connect()?;
    let Some(current) = get_task(&folder, name)? else {
        return Ok(false);
    };
    if let Some(expected_xml) = expected {
        let actual = unsafe { current.Xml().map_err(com)? }.to_string();
        if actual != expected_xml {
            return Err(conflict(
                "Scheduled task changed; preserving foreign definition.",
            ));
        }
    }
    if unsafe { current.State().map_err(com)? } == TASK_STATE_RUNNING {
        return Err(conflict(
            "Owned subscription task is running; live proxy was not stopped.",
        ));
    }
    unsafe { folder.DeleteTask(&BSTR::from(name), 0).map_err(com)? };
    Ok(true)
}

pub fn resolve_powershell() -> io::Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(root) = std::env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(root).join("PowerShell/7/pwsh.exe"));
    }
    if let Ok(output) = Command::new("where.exe")
        .arg("pwsh.exe")
        .creation_flags(0x08000000)
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let path = PathBuf::from(line.trim());
                if path.is_file() {
                    candidates.push(path);
                }
            }
        }
    }
    for path in candidates {
        let text = path.to_string_lossy();
        if text.contains("WindowsApps") || !path.is_file() {
            continue;
        }
        return Ok(path);
    }
    Err(conflict(
        "Subscription background startup requires native PowerShell 7.4+; Microsoft Store PowerShell is not a supported scheduled host.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_missing_owned_name_does_not_start_a_task() {
        let report = observe("codex-harness-subscriptions-missing-fixture").unwrap();
        assert!(report.is_none());
    }

    #[test]
    fn invalid_task_name_is_rejected_before_connect() {
        assert!(observe(&format!("a{}b", char::from(92))).is_err());
        assert!(register("a/b", "<Task></Task>", None).is_err());
    }
}
