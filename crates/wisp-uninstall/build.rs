fn main() {
    let root = std::path::PathBuf::from("../../assets");
    println!("cargo:rerun-if-changed={}", root.join("icon.ico").display());
    wisp_version::apply(
        &std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()),
        &root.join("icon.ico"),
        "WispUninstall.exe",
        "Wisp uninstaller",
    );
}
