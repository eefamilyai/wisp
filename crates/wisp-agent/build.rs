fn main() {
    let root = std::path::PathBuf::from("../../assets");
    let icon = root.join("icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    wisp_version::apply(
        &std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()),
        &icon,
        "Wisp.exe",
        "Wisp assistant",
    );
}
