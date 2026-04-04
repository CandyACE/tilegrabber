fn main() {
    // 读取 update_url 文件，将内容作为编译时常量注入
    let url = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("update_url"),
    )
    .unwrap_or_default();
    println!("cargo:rustc-env=TILEGRABBER_UPDATE_URL={}", url.trim());
    // 文件变化时重新编译
    println!("cargo:rerun-if-changed=update_url");

    tauri_build::build()
}
