// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)]

mod cleanup;
mod cmdline;
mod context;
mod status;
mod timesync;
mod virtio;

use cleanup::VsockCleanupGuard;
use cmdline::Args;
use context::KrunContext;

use clap::Parser;

fn main() -> Result<(), anyhow::Error> {
    // Gather the krun context from the command line arguments and configure the workload
    // accordingly.
    let ctx = KrunContext::try_from(Args::parse())?;

    // The guard removes vsock socket files on drop (normal exit) and on Ctrl-C.
    let _cleanup = VsockCleanupGuard::new(ctx.vsock_socket_paths())?;

    // Run the workload. If behaving properly, the main thread will not return from this
    // function.
    ctx.run()?;

    Ok(())
}
