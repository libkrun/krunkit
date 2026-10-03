// SPDX-License-Identifier: Apache-2.0

use crate::{cmdline::Args, status::RestfulUri, virtio::windows::attach_devices};
use anyhow::{anyhow, Context};
use std::{
    ffi::{c_char, c_void, CStr},
    fs::OpenOptions,
    io,
    os::windows::io::AsRawHandle,
    path::{Path, PathBuf},
    ptr,
};

type KrunObject = *mut c_void;
type KrunError = *mut c_void;
type KrunResult = u64;

const KRUN_SUCCESS: KrunResult = 0;
const KRUN_LOG_STYLE_AUTO: u32 = 0;
const KRUN_LOG_OPTION_ENV: u32 = 0;
const KRUN_LOG_OPTION_NO_ENV: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct KrunStr {
    data: *const c_char,
    len: usize,
}
impl KrunStr {
    fn new(value: &str) -> Self {
        Self {
            data: value.as_ptr().cast(),
            len: value.len(),
        }
    }
}

#[link(name = "krun.dll")]
unsafe extern "C" {
    fn krun_init_log(
        target: u64,
        level: u32,
        style: u32,
        options: u32,
        err_out: *mut KrunError,
    ) -> KrunResult;
    fn krun_mmio_device_manager_new() -> KrunObject;
    fn krun_mmio_device_manager_add(manager: KrunObject, device: KrunObject);
    fn krun_console_device_builder() -> KrunObject;
    fn krun_console_builder_add_default_console(
        builder: KrunObject,
        stdin: *mut c_void,
        stdout: *mut c_void,
        stderr: *mut c_void,
        err_out: *mut KrunError,
    ) -> KrunResult;
    fn krun_console_builder_build(builder: KrunObject, err_out: *mut KrunError) -> KrunObject;
    fn krun_payload_load_firmware(
        path: KrunStr,
        cmdline: KrunStr,
        err_out: *mut KrunError,
    ) -> KrunObject;
    fn krun_vmm_builder_new() -> KrunObject;
    fn krun_vmm_builder_vcpus(
        builder: *mut KrunObject,
        count: u8,
        err_out: *mut KrunError,
    ) -> KrunResult;
    fn krun_vmm_builder_ram_mib(
        builder: *mut KrunObject,
        mib: u32,
        err_out: *mut KrunError,
    ) -> KrunResult;
    fn krun_vmm_builder_payload(builder: *mut KrunObject, payload: KrunObject);
    fn krun_vmm_builder_devices(builder: *mut KrunObject, devices: KrunObject);
    fn krun_vmm_builder_acpi(
        builder: *mut KrunObject,
        enabled: bool,
        err_out: *mut KrunError,
    ) -> KrunResult;
    fn krun_vmm_builder_build(builder: *mut KrunObject, err_out: *mut KrunError) -> KrunObject;
    fn krun_vmm_run(vmm: KrunObject);
    fn krun_balloon_device_new(err_out: *mut KrunError) -> KrunObject;
    fn krun_rng_device_new(err_out: *mut KrunError) -> KrunObject;
    fn krun_error_result(error: KrunError) -> KrunResult;
    fn krun_error_destroy(error: KrunError);
    fn krun_result_name_cstr(result: KrunResult) -> *const c_char;
}

pub struct KrunContext {
    vmm: KrunObject,
    args: Args,
}

impl TryFrom<Args> for KrunContext {
    type Error = anyhow::Error;
    fn try_from(args: Args) -> Result<Self, Self::Error> {
        if args.cpus == 0 {
            return Err(anyhow!("vcpus must be a minimum of 1 (0 is invalid)"));
        }
        if args.memory == 0 {
            return Err(anyhow!("zero MiB RAM inputted (invalid)"));
        }
        if args
            .restful_uri
            .as_ref()
            .is_some_and(|uri| *uri != RestfulUri::None)
        {
            return Err(anyhow!("the RESTful service is not supported on Windows"));
        }
        init_logging(&args)?;
        let devices = unsafe { krun_mmio_device_manager_new() };
        if devices.is_null() {
            return Err(anyhow!("unable to create libkrun device manager"));
        }

        let console_builder = unsafe { krun_console_device_builder() };
        let mut error = ptr::null_mut();
        check_result(
            unsafe {
                krun_console_builder_add_default_console(
                    console_builder,
                    io::stdin().as_raw_handle().cast(),
                    io::stdout().as_raw_handle().cast(),
                    io::stderr().as_raw_handle().cast(),
                    &mut error,
                )
            },
            error,
            "unable to configure virtio console",
        )?;
        let mut error = ptr::null_mut();
        let console = unsafe { krun_console_builder_build(console_builder, &mut error) };
        check_object(console, error, "unable to build virtio console")?;
        unsafe { krun_mmio_device_manager_add(devices, console) };
        add_device(
            devices,
            unsafe { krun_balloon_device_new(ptr::null_mut()) },
            "balloon",
        )?;
        add_device(
            devices,
            unsafe { krun_rng_device_new(ptr::null_mut()) },
            "RNG",
        )?;

        unsafe { attach_devices(devices, &args.devices, check_object) }?;
        let firmware = args
            .firmware_path
            .clone()
            .or_else(get_firmware_path)
            .ok_or_else(|| anyhow!("can't find a firmware to load"))?;
        let mut error = ptr::null_mut();
        let payload = unsafe {
            krun_payload_load_firmware(
                KrunStr::new(path_str(&firmware, "firmware")?),
                KrunStr::new(""),
                &mut error,
            )
        };
        let payload = check_object(payload, error, "unable to load firmware")?;
        let mut builder = unsafe { krun_vmm_builder_new() };
        let mut error = ptr::null_mut();
        check_result(
            unsafe { krun_vmm_builder_vcpus(&mut builder, args.cpus, &mut error) },
            error,
            "unable to configure vCPUs",
        )?;
        let mut error = ptr::null_mut();
        check_result(
            unsafe { krun_vmm_builder_ram_mib(&mut builder, args.memory, &mut error) },
            error,
            "unable to configure RAM",
        )?;
        unsafe {
            krun_vmm_builder_payload(&mut builder, payload);
            krun_vmm_builder_devices(&mut builder, devices)
        };
        let mut error = ptr::null_mut();
        check_result(
            unsafe { krun_vmm_builder_acpi(&mut builder, true, &mut error) },
            error,
            "unable to enable ACPI",
        )?;
        let mut error = ptr::null_mut();
        let vmm = unsafe { krun_vmm_builder_build(&mut builder, &mut error) };
        Ok(Self {
            vmm: check_object(vmm, error, "unable to build libkrun VMM")?,
            args,
        })
    }
}

impl KrunContext {
    pub fn run(&self) -> Result<(), anyhow::Error> {
        if let Some(pidfile) = &self.args.pidfile {
            std::fs::write(pidfile, std::process::id().to_string())?;
        }
        unsafe { krun_vmm_run(self.vmm) };
        Ok(())
    }
}

fn add_device(
    manager: KrunObject,
    device: KrunObject,
    description: &str,
) -> Result<(), anyhow::Error> {
    if device.is_null() {
        return Err(anyhow!("unable to create {description} device"));
    }
    unsafe { krun_mmio_device_manager_add(manager, device) };
    Ok(())
}
fn init_logging(args: &Args) -> Result<(), anyhow::Error> {
    let (level, options) = args
        .krun_log_level
        .map(|level| (level, KRUN_LOG_OPTION_NO_ENV))
        .unwrap_or((3, KRUN_LOG_OPTION_ENV));
    let log_file;
    let handle = match &args.log_file {
        Some(path) => {
            log_file = OpenOptions::new().append(true).create(true).open(path)?;
            log_file.as_raw_handle() as u64
        }
        None => io::stderr().as_raw_handle() as u64,
    };
    let mut error = ptr::null_mut();
    check_result(
        unsafe { krun_init_log(handle, level, KRUN_LOG_STYLE_AUTO, options, &mut error) },
        error,
        "unable to initialize libkrun logging",
    )
}
fn check_object(
    object: KrunObject,
    error: KrunError,
    context: &str,
) -> Result<KrunObject, anyhow::Error> {
    check_error(error, context)?;
    if object.is_null() {
        Err(anyhow!("{context}"))
    } else {
        Ok(object)
    }
}
fn check_result(result: KrunResult, error: KrunError, context: &str) -> Result<(), anyhow::Error> {
    check_error(error, context)?;
    if result == KRUN_SUCCESS {
        Ok(())
    } else {
        Err(anyhow!("{context}: libkrun result {result:#x}"))
    }
}
fn check_error(error: KrunError, context: &str) -> Result<(), anyhow::Error> {
    if error.is_null() {
        return Ok(());
    }
    let result = unsafe { krun_error_result(error) };
    let name = unsafe {
        let name = krun_result_name_cstr(result);
        (!name.is_null()).then(|| CStr::from_ptr(name).to_string_lossy().into_owned())
    }
    .unwrap_or_else(|| format!("result {result:#x}"));
    unsafe { krun_error_destroy(error) };
    Err(anyhow!("{context}: {name}"))
}
fn path_str<'a>(path: &'a Path, description: &str) -> Result<&'a str, anyhow::Error> {
    path.to_str()
        .with_context(|| format!("{description} path is not valid UTF-8: {}", path.display()))
}
fn get_firmware_path() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;
    [
        directory.join("OVMF.fd"),
        directory.join("edk2/OVMF.fd"),
        directory.parent()?.join("edk2/OVMF.fd"),
        directory.parent()?.parent()?.join("edk2/OVMF.fd"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}
