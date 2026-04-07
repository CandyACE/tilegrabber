fn main() {
    // 优先从 update_url 文件读取；文件缺失/为空时不注入 cargo:rustc-env，
    // 让 option_env! 回退到进程环境变量（CI 通过 step env 注入）。
    let file_url = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("update_url"),
    )
    .unwrap_or_default();
    let file_url = file_url.trim();
    if !file_url.is_empty() {
        println!("cargo:rustc-env=TILEGRABBER_UPDATE_URL={}", file_url);
    }
    // 文件或环境变量变化时重新编译
    println!("cargo:rerun-if-changed=update_url");
    println!("cargo:rerun-if-env-changed=TILEGRABBER_UPDATE_URL");

    tauri_build::build()
}
