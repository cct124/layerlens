fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Tauri 的资源清单只附加到应用二进制；测试程序也需要 v6 才能加载 TaskDialogIndirect。
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
        // 应用及其二进制测试已有 Tauri 的完整清单，避免再嵌入同名资源。
        println!("cargo:rustc-link-arg-bin=layerlens=/MANIFEST:NO");
    }

    // 显式登记应用命令后，调用受 capability 的命令权限约束。
    let attributes =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "get_app_info",
            "workspace_action",
            "read_preview",
            "read_layers",
            "selection_request",
            "choose_psd_files",
        ]));
    if let Err(error) = tauri_build::try_build(attributes) {
        eprintln!("Tauri 构建配置失败：{error:#}");
        std::process::exit(1);
    }
}
