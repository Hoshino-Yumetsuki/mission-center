use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;
use std::sync::OnceLock;

fn cmdline_programs() -> &'static HashSet<&'static str> {
    static CMDLINE_PROGRAMS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    CMDLINE_PROGRAMS.get_or_init(|| parse_table(include_str!("exec_tables/cmdline_programs.txt")))
}

fn transparent_launchers() -> &'static HashSet<&'static str> {
    static TRANSPARENT_LAUNCHERS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    TRANSPARENT_LAUNCHERS
        .get_or_init(|| parse_table(include_str!("exec_tables/transparent_launchers.txt")))
}

fn opaque_launchers() -> &'static HashSet<&'static str> {
    static OPAQUE_LAUNCHERS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    OPAQUE_LAUNCHERS.get_or_init(|| parse_table(include_str!("exec_tables/opaque_launchers.txt")))
}

fn parse_table(contents: &'static str) -> HashSet<&'static str> {
    contents
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect()
}

pub(crate) fn parse_exec(exec: &str, env_path: &[String]) -> (Option<Rc<str>>, Vec<Rc<str>>) {
    let tokens: Vec<&str> = exec
        .split_ascii_whitespace()
        .map(|t| t.trim_matches('"'))
        .filter(|t| !t.is_empty())
        .collect();

    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];

        // Options ("-x", "--foo=bar") can precede the binary (e.g. `env -i my-app`);
        // they don't name the binary, but they DO belong in the returned argv when
        // the binary is eventually found. We just skip them for the binary search.
        if token.starts_with('-') {
            i += 1;
            continue;
        }

        let candidate = Path::new(token);
        let resolved = if candidate.is_absolute() {
            Some(candidate.to_owned())
        } else {
            env_path
                .iter()
                .map(|dir| Path::new(dir).join(token))
                .find(|p| p.exists())
        };

        let Some(resolved) = resolved.and_then(|p| p.canonicalize().ok()) else {
            i += 1;
            continue;
        };

        let file_name = resolved.file_name().unwrap_or_default().to_string_lossy();

        if opaque_launchers().contains(file_name.as_ref()) {
            return (None, Vec::new());
        }

        if transparent_launchers().contains(file_name.as_ref()) {
            i += 1;
            continue;
        }

        if cmdline_programs().contains(file_name.as_ref()) {
            return (None, Vec::new());
        }

        let exec_path = Rc::<str>::from(resolved.to_string_lossy().as_ref());
        let args: Vec<Rc<str>> = tokens[i..].iter().map(|t| Rc::<str>::from(*t)).collect();
        return (Some(exec_path), args);
    }

    (None, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_fake_path(test_name: &str, names: &[&str]) -> (PathBuf, Vec<String>) {
        let dir =
            std::env::temp_dir().join(format!("app_rummage_{}_{}", test_name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in names {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let env_path = vec![dir.to_string_lossy().into_owned()];
        (dir, env_path)
    }

    #[test]
    fn parse_exec_flatpak_run_yields_no_exec() {
        let (dir, env_path) = make_fake_path("flatpak_run", &["flatpak", "run"]);
        let (exec, args) = parse_exec("flatpak run org.example.App", &env_path);
        assert!(exec.is_none(), "flatpak launcher must not surface a binary");
        assert!(args.is_empty(), "no argv should be returned for flatpak");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_exec_snap_run_yields_no_exec() {
        let (dir, env_path) = make_fake_path("snap_run", &["snap", "run"]);
        let (exec, args) = parse_exec("snap run firefox", &env_path);
        assert!(exec.is_none(), "snap launcher must not surface a binary");
        assert!(args.is_empty(), "no argv should be returned for snap");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_exec_env_skips_to_real_binary() {
        let (dir, env_path) = make_fake_path("env_skip", &["env", "myapp"]);
        let (exec, args) = parse_exec("env LANG=C myapp --foo", &env_path);
        let exec = exec.expect("env should be skipped to reach myapp");
        assert!(exec.ends_with("myapp"), "got {exec}");
        let arg_strs: Vec<&str> = args.iter().map(|a| a.as_ref()).collect();
        assert_eq!(arg_strs, vec!["myapp", "--foo"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
