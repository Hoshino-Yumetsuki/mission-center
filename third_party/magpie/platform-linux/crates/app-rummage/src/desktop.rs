use std::collections::HashMap;
use std::io::{self, Read};
use std::path::Path;
use std::rc::Rc;

use crate::exec::parse_exec;
use crate::ApplicationEntry;

pub(crate) fn installed_apps_impl(
    xdg_data_dirs: &[String],
    env_path: &[String],
) -> HashMap<Rc<str>, ApplicationEntry> {
    let mut result = HashMap::new();
    let mut buffer = String::new();

    for dir in xdg_data_dirs {
        let dir = Path::new(&dir).join("applications");
        if dir.exists() {
            let dir = match dir.read_dir() {
                Ok(dc) => dc,
                Err(e) => {
                    log::warn!("Failed to read directory '{}': {:?}", dir.display(), e);
                    continue;
                }
            };

            for entry in dir {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        log::warn!("Failed to read entry directory entry: {:?}", e);
                        continue;
                    }
                };

                let path = entry.path();
                if path
                    .extension()
                    .map(|ext| ext == "desktop")
                    .unwrap_or(false)
                {
                    if let Ok(Some(app)) = extract_application_info(&path, env_path, &mut buffer) {
                        result.entry(app.id.clone()).or_insert(app);
                    }
                }
            }
        }
    }

    result
}

fn extract_application_info(
    path: &Path,
    env_path: &[String],
    buffer: &mut String,
) -> io::Result<Option<ApplicationEntry>> {
    buffer.clear();
    std::fs::File::options()
        .read(true)
        .open(path)?
        .read_to_string(buffer)?;

    let mut name: Option<&str> = None;
    let mut exec: Option<&str> = None;
    let mut icon: Option<&str> = None;
    let mut in_group = false;

    for line in buffer.split('\n') {
        if !in_group {
            if line == "[Desktop Entry]" {
                in_group = true;
            }
            continue;
        }

        // A new group header ends the [Desktop Entry] section.
        if line.starts_with('[') {
            break;
        }

        if let Some(rest) = line.strip_prefix("NoDisplay=") {
            if rest.trim() == "true" {
                return Ok(None);
            }
        } else if let Some(rest) = line.strip_prefix("Hidden=") {
            if rest.trim() == "true" {
                return Ok(None);
            }
        } else if let Some(rest) = line.strip_prefix("Type=") {
            if rest.trim() != "Application" {
                return Ok(None);
            }
        } else if let Some(rest) = line.strip_prefix("Name=") {
            name = Some(rest);
        } else if let Some(rest) = line.strip_prefix("Exec=") {
            exec = Some(rest);
        } else if let Some(rest) = line.strip_prefix("Icon=") {
            icon = Some(rest);
        }
    }

    let (Some(name), Some(exec)) = (name, exec) else {
        return Ok(None);
    };

    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let app_id = file_name
        .strip_suffix(".desktop")
        .map(Rc::<str>::from)
        .unwrap_or_else(|| Rc::<str>::from(file_name.as_ref()));

    let (exec_path, exec_args) = parse_exec(exec, env_path);

    Ok(Some(ApplicationEntry {
        id: app_id,
        name: Rc::from(name),
        exec: exec_path,
        exec_args,
        icon: icon.map(Rc::from),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_apps_user_dir_wins_on_duplicate_id() {
        // Build two fake XDG data dirs with the same desktop id and verify that
        // the one listed first (user dir) shadows the later one (system dir).
        let test_root =
            std::env::temp_dir().join(format!("app_rummage_xdg_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_root);

        let user_dir = test_root.join("user");
        let system_dir = test_root.join("system");
        std::fs::create_dir_all(user_dir.join("applications")).unwrap();
        std::fs::create_dir_all(system_dir.join("applications")).unwrap();

        std::fs::write(
            user_dir.join("applications/org.example.Demo.desktop"),
            "[Desktop Entry]\nType=Application\nName=Demo (User)\nExec=/usr/bin/true\n",
        )
        .unwrap();
        std::fs::write(
            system_dir.join("applications/org.example.Demo.desktop"),
            "[Desktop Entry]\nType=Application\nName=Demo (System)\nExec=/usr/bin/true\n",
        )
        .unwrap();

        let dirs = vec![
            user_dir.to_string_lossy().into_owned(),
            system_dir.to_string_lossy().into_owned(),
        ];
        let result = installed_apps_impl(&dirs, &[]);

        let app = result
            .get("org.example.Demo")
            .expect("duplicate-id app should still be present");
        assert_eq!(app.name.as_ref(), "Demo (User)");

        let _ = std::fs::remove_dir_all(&test_root);
    }

    #[test]
    fn installed_apps_honors_hidden_and_nodisplay() {
        let test_root =
            std::env::temp_dir().join(format!("app_rummage_hidden_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_root);

        let apps_dir = test_root.join("applications");
        std::fs::create_dir_all(&apps_dir).unwrap();

        std::fs::write(
            apps_dir.join("org.example.Hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=/usr/bin/true\nHidden=true\n",
        )
        .unwrap();
        std::fs::write(
            apps_dir.join("org.example.NoDisplay.desktop"),
            "[Desktop Entry]\nType=Application\nName=NoDisplay\nExec=/usr/bin/true\nNoDisplay=true\n",
        )
        .unwrap();
        std::fs::write(
            apps_dir.join("org.example.Visible.desktop"),
            "[Desktop Entry]\nType=Application\nName=Visible\nExec=/usr/bin/true\n",
        )
        .unwrap();

        let dirs = vec![test_root.to_string_lossy().into_owned()];
        let result = installed_apps_impl(&dirs, &[]);

        assert!(!result.contains_key("org.example.Hidden"));
        assert!(!result.contains_key("org.example.NoDisplay"));
        assert!(result.contains_key("org.example.Visible"));

        let _ = std::fs::remove_dir_all(&test_root);
    }

    #[test]
    fn test_available_applications() {
        let result = installed_apps_impl(crate::env::xdg_data_dirs(), crate::env::path());
        dbg!(&result);
    }
}
