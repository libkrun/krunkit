// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::cmdline::cstring_to_ptr;
use std::{
    ffi::{c_char, c_int, CString},
    os::unix::ffi::OsStrExt,
    path::Path,
};

#[link(name = "krun")]
unsafe extern "C" {
    fn krun_add_disk2(
        ctx_id: u32,
        block_id: *const c_char,
        path: *const c_char,
        format: u32,
        read_only: bool,
    ) -> i32;
    fn krun_add_vsock_port2(ctx_id: u32, port: u32, path: *const c_char, listen: bool) -> i32;
    fn krun_add_virtiofs4(
        ctx_id: u32,
        tag: *const c_char,
        path: *const c_char,
        shm_size: u64,
        read_only: bool,
        semantics: u32,
    ) -> i32;
    fn krun_set_console_output(ctx_id: u32, path: *const c_char) -> i32;
    fn krun_add_net_unixgram(
        ctx_id: u32,
        path: *const c_char,
        fd: c_int,
        mac: *const u8,
        features: u32,
        flags: u32,
    ) -> i32;
    fn krun_add_net_unixstream(
        ctx_id: u32,
        path: *const c_char,
        fd: c_int,
        mac: *const u8,
        features: u32,
        flags: u32,
    ) -> i32;
}

/// Each virito device configures itself with krun differently. This is used by each virtio device
/// to set their respective configurations with libkrun.
pub trait KrunContextSet {
    unsafe fn krun_ctx_set(&self, id: u32) -> Result<(), anyhow::Error>;
}

/// Configure the device in the krun context based on which underlying device is contained
impl KrunContextSet for VirtioDeviceConfig {
    unsafe fn krun_ctx_set(&self, id: u32) -> Result<(), anyhow::Error> {
        match self {
            Self::Blk(blk) => blk.krun_ctx_set(id),
            Self::Vsock(vsock) => vsock.krun_ctx_set(id),
            Self::Net(net) => net.krun_ctx_set(id),
            Self::Fs(fs) => fs.krun_ctx_set(id),
            Self::Serial(serial) => serial.krun_ctx_set(id),

            // virtio-input, virtio-gpu, and virtio-rng devices are currently not configured in
            // krun.
            _ => Ok(()),
        }
    }
}

impl KrunContextSet for BlkConfig {
    unsafe fn krun_ctx_set(&self, id: u32) -> Result<(), anyhow::Error> {
        let basename = match self.path.file_name() {
            Some(osstr) => osstr.to_str().unwrap_or("disk"),
            None => "disk",
        };
        let block_id_cstr = CString::new(basename).context("can't convert basename to cstring")?;
        let path_cstr = path_to_cstring(&self.path)?;

        if krun_add_disk2(
            id,
            block_id_cstr.as_ptr(),
            path_cstr.as_ptr(),
            self.format as u32,
            false,
        ) < 0
        {
            return Err(anyhow!(format!(
                "unable to set virtio-blk disk for {}",
                self.path.display()
            )));
        }

        Ok(())
    }
}

/// Set the krun console output to be written to the virtio-serial's log file.
impl KrunContextSet for SerialConfig {
    unsafe fn krun_ctx_set(&self, id: u32) -> Result<(), anyhow::Error> {
        let path = path_to_cstring(&self.log_file_path)?;

        if unsafe { krun_set_console_output(id, path.as_ptr()) } < 0 {
            return Err(anyhow!(
                "unable to set krun console output redirection to virtio-serial log file"
            ));
        }

        Ok(())
    }
}

/// Map the virtio-vsock's guest port and host path to enable the krun VM to communicate with the
/// socket on the host.
impl KrunContextSet for VsockConfig {
    unsafe fn krun_ctx_set(&self, id: u32) -> Result<(), anyhow::Error> {
        let path_cstr = path_to_cstring(&self.socket_url)?;

        // libkrun's `listen` parameter means "guest expects connections from host" which is true when VsockAction::Connect.
        if krun_add_vsock_port2(
            id,
            self.port,
            path_cstr.as_ptr(),
            self.action == VsockAction::Connect,
        ) < 0
        {
            return Err(anyhow!(format!(
                "unable to add vsock port {} for path {}",
                self.port,
                self.socket_url.display()
            )));
        }

        Ok(())
    }
}

impl KrunContextSet for NetConfig {
    unsafe fn krun_ctx_set(&self, id: u32) -> Result<(), anyhow::Error> {
        match &self.socket_type {
            SocketType::UnixGram => {
                let features = if self.socket_config.offloading {
                    COMPAT_NET_FEATURES
                } else {
                    0
                };

                let path = match &self.socket_config.path {
                    Some(path) => path_to_cstring(path)?,
                    None => path_to_cstring(&PathBuf::new())?,
                };

                let flags = if self.socket_config.send_vfkit_magic {
                    NET_FLAG_VFKIT
                } else {
                    0
                };

                if krun_add_net_unixgram(
                    id,
                    cstring_to_ptr(&path),
                    self.socket_config.fd.unwrap_or(-1),
                    self.mac_address.bytes().as_ptr(),
                    features,
                    flags,
                ) < 0
                {
                    // TODO(jakecorrenti): if this fails, we should display all of the values the
                    // user provided to the virtio-net cmdline
                    return Err(anyhow!(format!(
                        "virtio-net unable to add device with unix datagram backend {:#?}",
                        self.socket_config
                    )));
                }
            }
            SocketType::UnixStream => {
                let features = if self.socket_config.offloading {
                    COMPAT_NET_FEATURES
                } else {
                    0
                };

                let path = match &self.socket_config.path {
                    Some(path) => path_to_cstring(path)?,
                    None => path_to_cstring(&PathBuf::new())?,
                };

                if krun_add_net_unixstream(
                    id,
                    cstring_to_ptr(&path),
                    self.socket_config.fd.unwrap_or(-1),
                    self.mac_address.bytes().as_ptr(),
                    features,
                    0,
                ) < 0
                {
                    // TODO(jakecorrenti): if this fails, we should display all of the values the
                    // user provided to the virtio-net cmdline
                    return Err(anyhow!(format!(
                        "virtio-net unable to add device with unix stream backend {:#?}",
                        self.socket_config
                    )));
                }
            }
        }

        Ok(())
    }
}

impl KrunContextSet for FsConfig {
    unsafe fn krun_ctx_set(&self, id: u32) -> Result<(), anyhow::Error> {
        let shared_dir_cstr = path_to_cstring(&self.shared_dir)?;
        let mount_tag_cstr = path_to_cstring(&self.mount_tag)?;

        if krun_add_virtiofs4(
            id,
            mount_tag_cstr.as_ptr(),
            shared_dir_cstr.as_ptr(),
            0,
            false,
            self.permission_semantics.clone() as u32,
        ) < 0
        {
            return Err(anyhow!(format!(
                "unable to add virtiofs shared directory {} with mount tag {}",
                self.shared_dir.display(),
                self.mount_tag.display()
            )));
        }

        Ok(())
    }
}

/// Construct a NULL-terminated C string from a Rust Path object.
fn path_to_cstring(path: &Path) -> Result<CString, anyhow::Error> {
    let cstring = CString::new(path.as_os_str().as_bytes()).context(format!(
        "unable to convert path {} into NULL-terminated C string",
        path.display()
    ))?;

    Ok(cstring)
}
