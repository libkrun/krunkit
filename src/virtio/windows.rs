// SPDX-License-Identifier: Apache-2.0

use super::*;
use anyhow::{anyhow, Context, Result};
use std::{
    ffi::{c_char, c_void},
    path::Path,
    ptr,
};

type KrunObject = *mut c_void;
type KrunError = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct KrunStr {
    data: *const c_char,
    len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct KrunBytes {
    data: *const u8,
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
    fn krun_mmio_device_manager_add(manager: KrunObject, device: KrunObject);
    fn krun_block_device_new(
        id: KrunStr,
        path: KrunStr,
        format: u32,
        err_out: *mut KrunError,
    ) -> KrunObject;
    fn krun_fs_device_new(tag: KrunStr, host_path: KrunStr, err_out: *mut KrunError) -> KrunObject;
    fn krun_net_device_new_unixstream_path(
        id: KrunStr,
        path: KrunStr,
        mac: KrunBytes,
        features: u32,
        flags: u32,
        err_out: *mut KrunError,
    ) -> KrunObject;
    fn krun_vsock_device_new(cid: u64, tsi_features: u32, err_out: *mut KrunError) -> KrunObject;
    fn krun_vsock_device_add_unix_port(vsock: KrunObject, port: u32, path: KrunStr, listen: bool);
}

pub(crate) unsafe fn attach_devices(
    manager: *mut c_void,
    configs: &[VirtioDeviceConfig],
    check_object: fn(KrunObject, KrunError, &str) -> Result<KrunObject>,
) -> Result<()> {
    for (index, config) in configs.iter().enumerate() {
        let device = match config {
            VirtioDeviceConfig::Blk(block) => {
                let generated_id;
                let id = match block.serial.as_deref() {
                    Some(id) => id,
                    None => {
                        generated_id = format!("disk{index}");
                        &generated_id
                    }
                };
                let mut error = ptr::null_mut();
                let device = unsafe {
                    krun_block_device_new(
                        KrunStr::new(id),
                        KrunStr::new(path_str(&block.path, "block device")?),
                        block.format as u32,
                        &mut error,
                    )
                };
                check_object(device, error, "unable to create block device")?
            }
            VirtioDeviceConfig::Fs(fs) => {
                let mut error = ptr::null_mut();
                let device = unsafe {
                    krun_fs_device_new(
                        KrunStr::new(path_str(&fs.mount_tag, "virtio-fs mount tag")?),
                        KrunStr::new(path_str(&fs.shared_dir, "virtio-fs shared directory")?),
                        &mut error,
                    )
                };
                check_object(device, error, "unable to create virtio-fs device")?
            }
            VirtioDeviceConfig::Net(net) => {
                if net.socket_type != SocketType::UnixStream || net.socket_config.fd.is_some() {
                    return Err(anyhow!(
                        "Windows supports only path-based type=unixstream networking"
                    ));
                }
                let path =
                    net.socket_config.path.as_deref().ok_or_else(|| {
                        anyhow!("virtio-net type=unixstream requires path=<socket>")
                    })?;
                let id = format!("net{index}");
                let features = if net.socket_config.offloading {
                    COMPAT_NET_FEATURES
                } else {
                    0
                };
                let flags = if net.socket_config.send_vfkit_magic {
                    NET_FLAG_VFKIT
                } else {
                    0
                };
                let mut error = ptr::null_mut();
                let device = unsafe {
                    krun_net_device_new_unixstream_path(
                        KrunStr::new(&id),
                        KrunStr::new(path_str(path, "network socket")?),
                        KrunBytes {
                            data: net.mac_address.bytes().as_ptr(),
                            len: 6,
                        },
                        features,
                        flags,
                        &mut error,
                    )
                };
                check_object(device, error, "unable to create network device")?
            }
            VirtioDeviceConfig::Vsock(config) => {
                let mut error = ptr::null_mut();
                let device = unsafe { krun_vsock_device_new(3, 0, &mut error) };
                let device = check_object(device, error, "unable to create vsock device")?;
                unsafe {
                    krun_vsock_device_add_unix_port(
                        device,
                        config.port,
                        KrunStr::new(path_str(&config.socket_url, "vsock socket")?),
                        config.action == VsockAction::Connect,
                    )
                };
                device
            }
            _ => return Err(anyhow!("unsupported device on Windows: {config:?}")),
        };
        unsafe { krun_mmio_device_manager_add(manager, device) };
    }
    Ok(())
}

fn path_str<'a>(path: &'a Path, description: &str) -> Result<&'a str> {
    path.to_str()
        .with_context(|| format!("{description} path is not valid UTF-8: {}", path.display()))
}
