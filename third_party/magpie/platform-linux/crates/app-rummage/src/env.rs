use std::sync::OnceLock;

pub(crate) fn path() -> &'static [String] {
    static PATH: OnceLock<Vec<String>> = OnceLock::new();

    PATH.get_or_init(|| {
        let path = std::env::var_os("PATH").unwrap_or_else(|| {
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/snap/bin".into()
        });
        path.to_string_lossy()
            .split(':')
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
    })
    .as_slice()
}

pub(crate) fn home_dir() -> &'static str {
    static HOME_DIR: OnceLock<String> = OnceLock::new();

    HOME_DIR.get_or_init(|| {
        std::env::var("HOME").unwrap_or_else(|e| {
            log::error!("Failed to get HOME: {}. Falling back to `/`", e);
            "/".into()
        })
    })
}

pub(crate) fn xdg_data_dirs() -> &'static [String] {
    static XDG_DATA_DIRS: OnceLock<Vec<String>> = OnceLock::new();

    XDG_DATA_DIRS.get_or_init(|| {
        let xdg_data_home = match std::env::var("XDG_DATA_HOME") {
            Ok(dir) => dir.to_string(),
            Err(e) => {
                log::error!(
                    "Failed to get XDG_DATA_HOME: {e}. Falling back to `$HOME/.local/share`",
                );
                format!("{}/.local/share", home_dir())
            }
        };
        let mut out = vec![xdg_data_home];
        match std::env::var("XDG_DATA_DIRS") {
            Ok(dir) => out.append(&mut dir.split(':').map(String::from).collect()),
            Err(e) => {
                log::error!("Failed to get XDG_DATA_DIRS: {e}. Falling back to `/usr/share`");
                out.push("/usr/share".into())
            }
        };
        out
    })
}
