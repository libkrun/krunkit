// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)]

mod cmdline;
mod context;
mod status;
#[cfg(target_os = "macos")]
mod timesync;
mod virtio;

use cmdline::Args;
use context::KrunContext;

use clap::Parser;

fn main() -> Result<(), anyhow::Error> {
    // Gather the krun context from the command line arguments and configure the workload
    // accordingly.
    let ctx = KrunContext::try_from(Args::parse())?;

    // Run the workload. If behaving properly, the main thread will not return from this
    // function.
    ctx.run()?;

    Ok(())
}
