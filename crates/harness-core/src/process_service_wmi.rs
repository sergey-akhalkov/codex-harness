//! Local WMI call, used only in the bounded native service-create subprocess.
use std::{io, mem::ManuallyDrop, ptr::null_mut};
use windows::{
    Win32::System::{
        Com::*,
        Ole::{SafeArrayCreateVector, SafeArrayPutElement},
        Variant::*,
        Wmi::*,
    },
    core::{BSTR, IUnknown, Interface, PCWSTR, w},
};

fn error(error: windows::core::Error) -> io::Error {
    // COM descriptions can include command/environment values. Retain only HRESULT.
    io::Error::other(format!(
        "local WMI call failed: 0x{:08x}",
        error.code().0 as u32
    ))
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn object(services: &IWbemServices, name: &str) -> windows::core::Result<IWbemClassObject> {
    let mut object = None;
    unsafe {
        services.GetObject(
            &BSTR::from(name),
            WBEM_FLAG_RETURN_WBEM_COMPLETE,
            None,
            Some(&mut object),
            None,
        )?;
    }
    object.ok_or_else(windows::core::Error::from_thread)
}

fn put(object: &IWbemClassObject, name: PCWSTR, value: &VARIANT) -> windows::core::Result<()> {
    unsafe { object.Put(name, 0, value, 0) }
}

fn strings(values: &[String]) -> windows::core::Result<VARIANT> {
    let array = unsafe { SafeArrayCreateVector(VT_BSTR, 0, values.len() as u32) };
    if array.is_null() {
        return Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_OUTOFMEMORY,
        ));
    }
    // VARIANT owns the SAFEARRAY immediately, including on a later Put failure.
    let mut result = VARIANT::default();
    unsafe {
        (*result.Anonymous.Anonymous).vt = VT_ARRAY | VT_BSTR;
        (*result.Anonymous.Anonymous).Anonymous.parray = array;
        for (index, value) in values.iter().enumerate() {
            let value = BSTR::from(value.as_str());
            // For BSTR arrays SafeArrayPutElement takes the BSTR itself, not BSTR*.
            SafeArrayPutElement(array, &(index as i32), value.as_ptr().cast())?;
        }
    }
    Ok(result)
}

fn embedded(object: IWbemClassObject) -> windows::core::Result<VARIANT> {
    let mut result = VARIANT::default();
    unsafe {
        (*result.Anonymous.Anonymous).vt = VT_UNKNOWN;
        (*result.Anonymous.Anonymous).Anonymous.punkVal =
            ManuallyDrop::new(Some(object.cast::<IUnknown>()?));
    }
    Ok(result)
}

fn get_u32(object: &IWbemClassObject, name: PCWSTR) -> windows::core::Result<u32> {
    let mut value = VARIANT::default();
    unsafe {
        object.Get(name, 0, &mut value, None, None)?;
    }
    u32::try_from(&value)
}

pub(super) fn create(command: &[u16], directory: &str, environment: &[String]) -> io::Result<u32> {
    let call = || -> windows::core::Result<u32> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        let _apartment = Apartment;
        unsafe {
            CoInitializeSecurity(
                None,
                -1,
                None,
                None,
                RPC_C_AUTHN_LEVEL_DEFAULT,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
                None,
            )?;
        }
        let locator: IWbemLocator =
            unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)? };
        let services = unsafe {
            locator.ConnectServer(
                &BSTR::from("ROOT\\CIMV2"),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                WBEM_FLAG_CONNECT_USE_MAX_WAIT.0,
                &BSTR::new(),
                None,
            )?
        };
        unsafe {
            // RPC_C_AUTHN_WINNT = 10, RPC_C_AUTHZ_NONE = 0; current local token only.
            CoSetProxyBlanket(
                &services,
                10,
                0,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )?;
        }
        let startup = unsafe { object(&services, "Win32_ProcessStartup")?.SpawnInstance(0)? };
        put(&startup, w!("ShowWindow"), &VARIANT::from(0_i32))?;
        put(&startup, w!("CreateFlags"), &VARIANT::from(0x09000400_i32))?;
        put(&startup, w!("EnvironmentVariables"), &strings(environment)?)?;
        let process = object(&services, "Win32_Process")?;
        let mut signature = None;
        unsafe {
            process.GetMethod(w!("Create"), 0, &mut signature, null_mut())?;
        }
        let input = unsafe {
            signature
                .ok_or_else(windows::core::Error::from_thread)?
                .SpawnInstance(0)?
        };
        put(
            &input,
            w!("CommandLine"),
            &VARIANT::from(BSTR::from_wide(command)),
        )?;
        put(&input, w!("CurrentDirectory"), &VARIANT::from(directory))?;
        put(&input, w!("ProcessStartupInformation"), &embedded(startup)?)?;
        let mut output = None;
        unsafe {
            services.ExecMethod(
                &BSTR::from("Win32_Process"),
                &BSTR::from("Create"),
                WBEM_FLAG_RETURN_WBEM_COMPLETE,
                None,
                &input,
                Some(&mut output),
                None,
            )?;
        }
        let output = output.ok_or_else(windows::core::Error::from_thread)?;
        let result = get_u32(&output, w!("ReturnValue"))?;
        if result != 0 {
            // ReturnValue is not HRESULT; do not turn it into a success HRESULT.
            return Err(windows::core::Error::from_hresult(
                windows::core::HRESULT::from_win32(result),
            ));
        }
        get_u32(&output, w!("ProcessId"))
    };
    call().map_err(error)
}
