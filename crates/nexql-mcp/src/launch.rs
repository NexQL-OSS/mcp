// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Absolute launch commands for desktop MCP clients.
//!
//! Claude Desktop is started from the Dock or Start menu, not a shell. On
//! macOS it does not inherit the login shell's `PATH` (nvm, Homebrew,
//! `~/.cargo/bin`), so a bare `nexql-mcp` or `npx` fails with `spawn ENOENT`.
//! Configs written for it name the program by absolute path, and an `npx`
//! launch also gets a `PATH` that contains `node`.

use std::path::{Path, PathBuf};

/// Clients that need an absolute command. Project-level configs that teams
/// commit (`.mcp.json`, `.vscode/mcp.json`, …) keep the portable bare name.
pub fn needs_absolute_command(client_key: &str) -> bool {
    matches!(client_key, "claude" | "claude-desktop")
}

/// A resolved launch: program, its arguments, and extra environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// True when a bare `nexql-mcp` in a client config starts an installed
/// copy. npx, pnpm dlx and bunx put their throwaway copy on `PATH` while
/// they run, so that copy does not count.
pub fn bare_command_resolves() -> bool {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    find_installed_on_path(&path_var, &std::env::temp_dir()).is_some()
}

/// Resolve `command` (`nexql-mcp` or `npx`) to an absolute path using the
/// current `PATH` and executable. `nexql-mcp` that only exists in a
/// throwaway npx cache falls back to an absolute `npx -y nexql-mcp@latest`. When
/// nothing resolves, the launch is returned unchanged.
pub fn absolute_launch(command: &str, args: &[String]) -> Launch {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let current = std::env::current_exe().ok();
    absolute_launch_with(
        command,
        args,
        &path_var,
        current.as_deref(),
        &std::env::temp_dir(),
    )
}

fn absolute_launch_with(
    command: &str,
    args: &[String],
    path_var: &std::ffi::OsStr,
    current_exe: Option<&Path>,
    temp: &Path,
) -> Launch {
    let unchanged = Launch {
        command: command.to_owned(),
        args: args.to_vec(),
        env: Vec::new(),
    };
    match command {
        "nexql-mcp" => {
            if let Some(found) = find_installed_on_path(path_var, temp).or_else(|| {
                current_exe
                    .filter(|p| is_stable_install(p, temp))
                    .map(Path::to_path_buf)
            }) {
                return Launch {
                    command: found.display().to_string(),
                    ..unchanged
                };
            }
            let mut npx_args = vec!["-y".to_owned(), "nexql-mcp@latest".to_owned()];
            npx_args.extend_from_slice(args);
            match npx_launch(&npx_args, path_var) {
                Some(launch) => launch,
                None => unchanged,
            }
        }
        "npx" => npx_launch(args, path_var).unwrap_or(unchanged),
        _ => unchanged,
    }
}

/// `npx` is a `#!/usr/bin/env node` script, so the client also needs `node`
/// on its `PATH`. Node installs put both in the same directory. Windows apps
/// inherit the user's `PATH`, so only the program is made absolute there.
fn npx_launch(args: &[String], path_var: &std::ffi::OsStr) -> Option<Launch> {
    let npx = find_on_path("npx", path_var)?;
    if cfg!(windows) {
        return Some(Launch {
            command: npx.display().to_string(),
            args: args.to_vec(),
            env: Vec::new(),
        });
    }
    let dir = npx.parent()?;
    let mut dirs = vec![dir.to_path_buf()];
    dirs.extend(
        ["/usr/local/bin", "/usr/bin", "/bin"]
            .iter()
            .map(PathBuf::from)
            .filter(|d| d != dir),
    );
    let path = std::env::join_paths(dirs)
        .ok()?
        .to_string_lossy()
        .into_owned();
    Some(Launch {
        command: npx.display().to_string(),
        args: args.to_vec(),
        env: vec![("PATH".to_owned(), path)],
    })
}

fn find_on_path(name: &str, path_var: &std::ffi::OsStr) -> Option<PathBuf> {
    candidates_on_path(name, path_var).next()
}

/// The first `nexql-mcp` on `PATH` that is an install, skipping cache copies
/// that npx and friends put in front of it.
fn find_installed_on_path(path_var: &std::ffi::OsStr, temp: &Path) -> Option<PathBuf> {
    candidates_on_path("nexql-mcp", path_var).find(|p| is_stable_install(p, temp))
}

fn candidates_on_path(name: &str, path_var: &std::ffi::OsStr) -> impl Iterator<Item = PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        vec![
            format!("{name}.exe"),
            format!("{name}.cmd"),
            name.to_owned(),
        ]
    } else {
        vec![name.to_owned()]
    };
    std::env::split_paths(path_var)
        .filter(|dir| dir.is_absolute())
        .flat_map(move |dir| names.clone().into_iter().map(move |n| dir.join(n)))
        .filter(|candidate| candidate.is_file())
}

/// True for an installed `nexql-mcp`. False for other binaries (test
/// harnesses) and for copies that disappear: the npx, pnpm dlx and bunx
/// caches and temp directories.
fn is_stable_install(exe: &Path, temp: &Path) -> bool {
    let named = exe
        .file_stem()
        .is_some_and(|stem| stem.eq_ignore_ascii_case("nexql-mcp"));
    let text = exe.to_string_lossy().to_ascii_lowercase();
    let parts: Vec<String> = exe
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();
    let pnpm_dlx = text.contains("pnpm") && parts.iter().any(|p| p == "dlx");
    let bunx = parts.iter().any(|p| p.starts_with("bunx-"));
    named
        && exe.is_absolute()
        && !text.contains("_npx")
        && !text.contains("/.npm/")
        && !text.contains("\\npm-cache\\")
        && !pnpm_dlx
        && !bunx
        && !exe.starts_with(temp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn bin_dir(root: &Path, name: &str, files: &[&str]) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        for f in files {
            std::fs::write(dir.join(f), "").unwrap();
        }
        dir
    }

    /// Test bin directories live under the real temp dir, so the temp-dir
    /// check is pointed elsewhere.
    fn no_temp() -> &'static Path {
        Path::new("/nonexistent-temp")
    }

    fn path_of(dirs: &[&Path]) -> OsString {
        std::env::join_paths(dirs).unwrap()
    }

    #[test]
    fn only_claude_desktop_gets_an_absolute_command() {
        assert!(needs_absolute_command("claude-desktop"));
        assert!(needs_absolute_command("claude"));
        assert!(!needs_absolute_command("claude-code"));
        assert!(!needs_absolute_command("vscode"));
    }

    #[test]
    fn nexql_mcp_on_path_is_used_by_absolute_path() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = bin_dir(tmp.path(), "bin", &["nexql-mcp"]);
        let args = vec!["--profile".to_owned(), "shop".to_owned()];
        let launch = absolute_launch_with("nexql-mcp", &args, &path_of(&[&bin]), None, no_temp());
        assert_eq!(launch.command, bin.join("nexql-mcp").display().to_string());
        assert_eq!(launch.args, args);
        assert!(launch.env.is_empty());
    }

    #[test]
    fn stable_current_exe_is_used_when_not_on_path() {
        let installed = if cfg!(windows) {
            r"C:\Program Files\nexql\nexql-mcp.exe"
        } else {
            "/opt/nexql/bin/nexql-mcp"
        };
        let launch = absolute_launch_with(
            "nexql-mcp",
            &[],
            &OsString::new(),
            Some(Path::new(installed)),
            no_temp(),
        );
        assert_eq!(launch.command, installed);
    }

    #[cfg(unix)]
    #[test]
    fn npx_cache_copy_falls_back_to_absolute_npx_with_node_on_path() {
        let tmp = tempfile::tempdir().unwrap();
        let node_bin = bin_dir(tmp.path(), "node/bin", &["npx", "node"]);
        let exe = Path::new("/home/u/.npm/_npx/abc/node_modules/.bin/nexql-mcp");
        let args = vec!["--profile".to_owned(), "shop".to_owned()];
        let launch = absolute_launch_with(
            "nexql-mcp",
            &args,
            &path_of(&[&node_bin]),
            Some(exe),
            no_temp(),
        );
        assert_eq!(launch.command, node_bin.join("npx").display().to_string());
        assert_eq!(launch.args, ["-y", "nexql-mcp@latest", "--profile", "shop"]);
        let (key, value) = &launch.env[0];
        assert_eq!(key, "PATH");
        assert!(
            value.starts_with(&node_bin.display().to_string()),
            "{value}"
        );
    }

    #[test]
    fn other_binaries_are_not_mistaken_for_an_install() {
        let exe = Path::new("/repo/target/debug/deps/nexql_mcp-0123abcd");
        let launch = absolute_launch_with("nexql-mcp", &[], &OsString::new(), Some(exe), no_temp());
        assert_eq!(launch.command, "nexql-mcp");
    }

    #[test]
    fn nothing_found_leaves_the_launch_unchanged() {
        let launch = absolute_launch_with("nexql-mcp", &[], &OsString::new(), None, no_temp());
        assert_eq!(launch.command, "nexql-mcp");
        assert!(launch.env.is_empty());
        let launch = absolute_launch_with("npx", &["-y".into()], &OsString::new(), None, no_temp());
        assert_eq!(launch.command, "npx");
    }

    #[cfg(unix)]
    #[test]
    fn npx_cache_copy_on_path_is_skipped_for_a_later_install() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = bin_dir(
            tmp.path(),
            ".npm/_npx/abc/node_modules/.bin",
            &["nexql-mcp"],
        );
        let installed = bin_dir(tmp.path(), "cargo/bin", &["nexql-mcp"]);
        let path = path_of(&[&cache, &installed]);
        assert_eq!(
            find_installed_on_path(&path, no_temp()),
            Some(installed.join("nexql-mcp"))
        );
        let launch = absolute_launch_with("nexql-mcp", &[], &path, None, no_temp());
        assert_eq!(
            launch.command,
            installed.join("nexql-mcp").display().to_string()
        );
    }

    #[cfg(unix)]
    #[test]
    fn only_a_cache_copy_on_path_is_not_an_install() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = bin_dir(
            tmp.path(),
            ".npm/_npx/abc/node_modules/.bin",
            &["nexql-mcp"],
        );
        assert_eq!(find_installed_on_path(&path_of(&[&cache]), no_temp()), None);
        // The real temp dir counts as throwaway too.
        let bin = bin_dir(tmp.path(), "bin", &["nexql-mcp"]);
        assert_eq!(find_installed_on_path(&path_of(&[&bin]), tmp.path()), None);
    }

    #[cfg(unix)]
    #[test]
    fn package_runner_caches_are_not_installs() {
        for exe in [
            "/home/u/.npm/_npx/abc/node_modules/.bin/nexql-mcp",
            "/home/u/.cache/pnpm/dlx/abc/def/node_modules/.bin/nexql-mcp",
            "/Users/u/Library/Caches/pnpm/dlx/abc/node_modules/.bin/nexql-mcp",
            "/var/folders/xy/T/bunx-501-nexql-mcp@latest/node_modules/.bin/nexql-mcp",
        ] {
            assert!(!is_stable_install(Path::new(exe), no_temp()), "{exe}");
        }
        for exe in [
            "/usr/local/bin/nexql-mcp",
            "/home/u/.cargo/bin/nexql-mcp",
            "/home/u/.local/share/pnpm/nexql-mcp",
        ] {
            assert!(is_stable_install(Path::new(exe), no_temp()), "{exe}");
        }
    }
}
