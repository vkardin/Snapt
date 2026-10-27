//! Packs the Snapt binary named in `SNAPT_EXE` into the installer, and takes
//! the version from the root Cargo.toml so both always match.

use std::{env, fs, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    println!("cargo:rerun-if-env-changed=SNAPT_EXE");
    let payload = match env::var_os("SNAPT_EXE") {
        Some(path) => {
            println!("cargo:rerun-if-changed={}", PathBuf::from(&path).display());
            let exe = fs::read(&path).expect("SNAPT_EXE points to a readable file");
            miniz_oxide::deflate::compress_to_vec(&exe, 10)
        }
        // Builds without a payload still run, to check the code and draw the window.
        None => Vec::new(),
    };
    fs::write(out.join("payload.bin"), payload).unwrap();

    let manifest = fs::read_to_string("../Cargo.toml").unwrap();
    let version = manifest
        .lines()
        .find_map(|l| l.strip_prefix("version = \""))
        .and_then(|v| v.strip_suffix('"'))
        .expect("the root Cargo.toml has a version");
    println!("cargo:rustc-env=SNAPT_VERSION={version}");
    println!("cargo:rerun-if-changed=../Cargo.toml");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        #[cfg(windows)]
        {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("../assets/icons/icon.ico")
                .set_manifest_file("app.manifest")
                .set("FileDescription", "Snapt Setup")
                .set("ProductName", "Snapt")
                .set("ProductVersion", version)
                .set("LegalCopyright", "Copyright (c) vkardin. MIT License.");
            res.compile()
                .expect("failed to embed the icon and manifest");
        }
    }
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=../assets/icons/icon.ico");
}
