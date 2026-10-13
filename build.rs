// SPDX-License-Identifier: Apache-2.0

fn main() {
    #[cfg(target_os = "macos")]
    {
        // Must match the default PREFIX in the Makefile.
        const DEFAULT_PREFIX: &str = "/usr/local";

        let prefix = std::env::var("PREFIX").unwrap_or(DEFAULT_PREFIX.to_string());
        println!("cargo:rustc-link-search={prefix}/lib");

        println!("cargo:rerun-if-env-changed=PREFIX");
    }

    #[cfg(target_os = "windows")]
    {
        let lib_dir = std::env::var("LIBKRUN_LIB_DIR")
            .expect("LIBKRUN_LIB_DIR must point to the directory containing krun.dll.lib");
        println!("cargo:rustc-link-search={lib_dir}");

        println!("cargo:rerun-if-env-changed=LIBKRUN_LIB_DIR");
    }
}
