fn main() {
    // Expose the build-time target triple as `WOOM_TARGET_TRIPLE` so
    // `src/rtk.rs` can locate the bundled `rtk-<triple>` sidecar at
    // runtime (Tauri keeps the triple suffix when laying out externalBin
    // in dev; release bundles also include the triple-suffixed name
    // alongside the plain alias).
    if let Ok(triple) = std::env::var("TARGET") {
        println!("cargo:rustc-env=WOOM_TARGET_TRIPLE={triple}");
    }
    println!("cargo:rerun-if-env-changed=TARGET");
    tauri_build::build()
}
