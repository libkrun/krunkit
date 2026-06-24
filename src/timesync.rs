// SPDX-License-Identifier: Apache-2.0

use crate::virtio::VsockConfig;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use core_foundation::runloop::{
    __CFRunLoopSource, kCFRunLoopCommonModes, CFRunLoopAddSource, CFRunLoopGetCurrent, CFRunLoopRun,
};
use objc2_io_kit::{
    io_object_t, io_service_t, kIOMessageSystemHasPoweredOn, kIOMessageSystemWillPowerOn,
    kIOMessageSystemWillSleep, IONotificationPort, IORegisterForSystemPower,
};
use std::{
    ffi::c_void,
    sync::mpsc::{channel, Receiver, Sender},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Sleep,
    Wake,
}

#[allow(non_upper_case_globals)]
extern "C-unwind" fn power_callback(
    refcon: *mut c_void,
    _service: io_service_t,
    message_type: u32,
    _message_argument: *mut c_void,
) {
    let tx = unsafe { &*(refcon as *mut Sender<Activity>) };
    log::debug!("Power callback called: {:X?}", message_type);
    let activity = match message_type {
        kIOMessageSystemWillSleep => Some(Activity::Sleep),
        kIOMessageSystemWillPowerOn | kIOMessageSystemHasPoweredOn => Some(Activity::Wake),
        _ => {
            log::debug!("Unknown message type: {:X?}", message_type);
            None
        }
    };
    if let Some(activity) = activity {
        if let Err(e) = tx.send(activity) {
            log::error!("Failed to send activity: {e}");
        }
    }
}

pub fn start_power_monitor() -> Receiver<Activity> {
    let (tx, rx) = channel::<Activity>();
    std::thread::spawn(move || unsafe {
        let tx_ptr = Box::into_raw(Box::new(tx));
        let mut notifier_port: *mut IONotificationPort = std::ptr::null_mut();
        let mut notifier_object: io_object_t = 0;

        let root_port = IORegisterForSystemPower(
            tx_ptr as *mut c_void,
            &mut notifier_port,
            Some(power_callback),
            &mut notifier_object,
        );
        if root_port == 0 {
            log::error!("Failed to register for system power notifications");
            return;
        }
        let run_loop_source = IONotificationPort::run_loop_source(notifier_port).unwrap();
        CFRunLoopAddSource(
            CFRunLoopGetCurrent(),
            std::ptr::from_ref(&*run_loop_source) as *mut __CFRunLoopSource,
            kCFRunLoopCommonModes,
        );
        CFRunLoopRun();
    });
    rx
}

fn sync_time(socket_url: PathBuf) {
    let mut stream = match UnixStream::connect(&socket_url) {
        Ok(stream) => stream,
        Err(e) => {
            log::error!(
                "Failed to connect to timesync socket {:?}: {}",
                socket_url,
                e
            );
            return;
        }
    };

    let time_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let cmd =
        format!("{{\"execute\": \"guest-set-time\", \"arguments\":{{\"time\": {time_ns}}}}}\n");

    if let Err(e) = stream.write_all(cmd.as_bytes()) {
        log::error!("Failed to write to timesync socket: {}", e);
        return;
    }

    let mut reader = BufReader::new(&stream);
    let mut response = String::new();
    match reader.read_line(&mut response) {
        Ok(_) => {
            log::info!("Time synced to {time_ns}");
        }
        Err(e) => {
            log::error!("Failed to read qemu-guest-agent response: {e}");
        }
    }
}

pub fn timesync_listener(vsock_config: VsockConfig) {
    let rx = start_power_monitor();
    for activity in rx {
        match activity {
            Activity::Sleep => {
                log::debug!("System is going to sleep");
            }
            Activity::Wake => {
                log::debug!("System is waking up. Syncing time...");
                sync_time(vsock_config.socket_url.clone());
            }
        }
    }
}
