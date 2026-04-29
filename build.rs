fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        println!("cargo:rustc-link-arg=-Wl,-Bdynamic");
        println!("cargo:rustc-link-arg=-lGL");
    }

    #[cfg(target_os = "macos")]
    {
        let out_dir = std::env::var("OUT_DIR")?;
        let src_dir = std::path::PathBuf::from(&out_dir).join("src");
        std::fs::create_dir_all(&src_dir)?;
        let config_path = src_dir.join("config.rs");
        std::fs::write(
            &config_path,
            r#"pub static VERSION: &str = "0.6.1-dev";
pub static GETTEXT_PACKAGE: &str = "missioncenter";
pub static LOCALEDIR: &str = "/opt/homebrew/share/locale";
pub static PKGDATADIR: &str = env!("CARGO_MANIFEST_DIR");
"#,
        )?;
        println!("cargo:rustc-env=BUILD_ROOT={}", out_dir);
    }

    Ok(())
}
