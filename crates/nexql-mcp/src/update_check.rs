// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Tells the user, through the model, when a newer nexql-mcp is released.
//!
//! At most once a day the server asks the npm registry for the latest
//! version and caches the answer in `~/.local/share/nexql-mcp/update-check.json`.
//! When that is newer than this binary, the server instructions (from the
//! cache, at `initialize`) and the first successful tool result carry a
//! notice with the upgrade command for the way this copy was installed, so
//! the model can tell the user or, with their OK, run it. The check never
//! delays startup and fails silently. Off with `--no-update-check`
//! (`NEXQL_MCP_NO_UPDATE_CHECK`), `update_check = false`, `DO_NOT_TRACK=1`,
//! or under CI.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const REGISTRY_URL: &str = "https://registry.npmjs.org/nexql-mcp/latest";
const RELEASES_URL: &str = "https://github.com/NexQL-OSS/mcp/releases";
const BUNDLE_URL: &str = "https://nexql-mcp.astrx.dev/install";
const INSTALL_SH: &str = "https://raw.githubusercontent.com/NexQL-OSS/mcp/main/scripts/install.sh";
const INSTALL_PS1: &str =
    "https://raw.githubusercontent.com/NexQL-OSS/mcp/main/scripts/install.ps1";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(3);

/// How this copy of nexql-mcp was installed, read from the executable's path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// npx, `pnpm dlx` or bunx cache: the client's next launch fetches it.
    Npx,
    NpmGlobal,
    Volta,
    PnpmGlobal,
    BunGlobal,
    /// `npm install nexql-mcp` in a project; the path is the project root.
    NpmProject(PathBuf),
    Cargo,
    UvTool,
    Uvx,
    Pipx,
    /// pip into a virtualenv; the path is the venv root.
    PipVenv(PathBuf),
    Homebrew,
    ClaudeExtension,
    Docker,
    /// install.sh / install.ps1, or a binary put there by hand; the path is
    /// its directory, which the install script can write to again.
    Standalone(PathBuf),
    /// A `cargo build` binary under `target/`: no notice.
    DevBuild,
}

/// Detect the install from `exe`. `exists` is `Path::exists`, injected for tests.
pub fn detect(exe: &Path, exists: &dyn Fn(&Path) -> bool) -> Install {
    let p = exe.to_string_lossy().replace('\\', "/").to_lowercase();
    let has = |s: &str| p.contains(s);
    let bin_parent = exe
        .parent()
        .filter(|d| {
            d.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("bin") || n.eq_ignore_ascii_case("scripts"))
        })
        .and_then(Path::parent);

    if has("/target/") && (has("/debug/") || has("/release/")) {
        return Install::DevBuild;
    }
    if exists(Path::new("/.dockerenv")) {
        return Install::Docker;
    }
    if has("/claude extensions/") {
        return Install::ClaudeExtension;
    }
    if has("/_npx/") || has("/pnpm/dlx/") || has("/bunx-") {
        return Install::Npx;
    }
    if has("/node_modules/") {
        if has("/.volta/") {
            return Install::Volta;
        }
        if has("/.bun/install/global/") {
            return Install::BunGlobal;
        }
        if has("/pnpm/global/") {
            return Install::PnpmGlobal;
        }
        if has("/lib/node_modules/") || has("/npm/node_modules/") {
            return Install::NpmGlobal;
        }
        // Outermost `node_modules` (pnpm nests a `.pnpm/…/node_modules`).
        if let Some(root) = exe
            .ancestors()
            .filter(|a| a.file_name().is_some_and(|n| n == "node_modules"))
            .last()
            .and_then(Path::parent)
        {
            return Install::NpmProject(root.to_path_buf());
        }
    }
    if has("/uv/tools/") {
        return Install::UvTool;
    }
    if has("/uv/archive-v") {
        return Install::Uvx;
    }
    if has("/pipx/venvs/") {
        return Install::Pipx;
    }
    if has("/cellar/nexql-mcp/") {
        return Install::Homebrew;
    }
    if let Some(root) = bin_parent {
        // `cargo install` keeps its ledger next to `bin/`.
        if exists(&root.join(".crates.toml")) || exists(&root.join(".crates2.json")) {
            return Install::Cargo;
        }
        if exists(&root.join("pyvenv.cfg")) {
            return Install::PipVenv(root.to_path_buf());
        }
    }
    Install::Standalone(exe.parent().map(Path::to_path_buf).unwrap_or_default())
}

/// How to upgrade an [`Install`], in words the model can pass on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upgrade {
    pub via: &'static str,
    pub command: Option<String>,
    pub note: Option<String>,
}

pub fn upgrade_for(install: &Install) -> Option<Upgrade> {
    let cmd = |via, command: String| Upgrade {
        via,
        command: Some(command),
        note: None,
    };
    Some(match install {
        Install::DevBuild => return None,
        Install::Npx => Upgrade {
            via: "npx",
            command: None,
            note: Some(
                "Nothing to install: npx fetches the new version the next time the MCP client \
                 starts this server, if the client's MCP config runs `nexql-mcp@latest`. If it \
                 runs `npx -y nexql-mcp` or a fixed version such as `nexql-mcp@0.5.0`, change \
                 that argument to `nexql-mcp@latest` in the client's MCP config first."
                    .into(),
            ),
        },
        Install::NpmGlobal => cmd("npm (global)", "npm install -g nexql-mcp@latest".into()),
        Install::Volta => cmd("Volta", "volta install nexql-mcp@latest".into()),
        Install::PnpmGlobal => cmd("pnpm (global)", "pnpm add -g nexql-mcp@latest".into()),
        Install::BunGlobal => cmd("bun (global)", "bun add -g nexql-mcp@latest".into()),
        Install::NpmProject(root) => cmd(
            "npm (project dependency)",
            format!(
                "npm install --prefix \"{}\" nexql-mcp@latest",
                root.display()
            ),
        ),
        Install::Cargo => Upgrade {
            via: "cargo install",
            command: Some("cargo install nexql-mcp --locked".into()),
            note: Some("It builds from source and needs clang.".into()),
        },
        Install::UvTool => cmd("uv tool", "uv tool upgrade nexql-mcp".into()),
        Install::Uvx => Upgrade {
            via: "uvx",
            command: None,
            note: Some(
                "Nothing to install: uvx fetches the new version the next time the MCP client \
                 starts this server, if the client's MCP config runs `uvx nexql-mcp@latest`. If \
                 it runs `uvx nexql-mcp` without `@latest`, change it in the client's MCP config \
                 first."
                    .into(),
            ),
        },
        Install::Pipx => cmd("pipx", "pipx upgrade nexql-mcp".into()),
        Install::PipVenv(root) => {
            let python = if cfg!(windows) {
                root.join("Scripts").join("python.exe")
            } else {
                root.join("bin").join("python")
            };
            cmd(
                "pip (virtualenv)",
                format!("\"{}\" -m pip install -U nexql-mcp", python.display()),
            )
        }
        Install::Homebrew => cmd("Homebrew", "brew upgrade nexql-mcp".into()),
        Install::ClaudeExtension => Upgrade {
            via: "the Claude Desktop extension",
            command: None,
            note: Some(format!(
                "Download the new bundle from {BUNDLE_URL} and open it to replace this \
                 extension, or update it in Claude's Settings → Extensions if an update is \
                 offered there. Then restart Claude."
            )),
        },
        Install::Docker => Upgrade {
            via: "Docker",
            command: Some("docker pull ghcr.io/nexql-oss/mcp:latest".into()),
            note: Some(
                "If the client's MCP config runs a fixed image tag, change it to `:latest`.".into(),
            ),
        },
        Install::Standalone(dir) => cmd(
            "the install script or a downloaded binary",
            if cfg!(windows) {
                format!(
                    "$env:NEXQL_MCP_INSTALL_DIR = \"{}\"; irm {INSTALL_PS1} | iex",
                    dir.display()
                )
            } else {
                format!(
                    "curl -fsSL {INSTALL_SH} | NEXQL_MCP_INSTALL_DIR=\"{}\" bash",
                    dir.display()
                )
            },
        ),
    })
}

/// The text the model reads, plus the same facts as structured content.
/// `remote` is an HTTP server on another machine: the user cannot run the
/// upgrade, whoever runs the server must.
pub fn notice(
    current: &Version,
    latest: &Version,
    upgrade: &Upgrade,
    remote: bool,
) -> (String, Value) {
    let release = format!("{RELEASES_URL}/tag/v{latest}");
    let mut text = format!(
        "nexql-mcp {latest} is available; this server runs {current}, installed via {}. \
         Tell the user once, briefly, after answering (skip this if you already told them).",
        upgrade.via
    );
    if remote {
        text.push_str(" This server runs on another machine, so whoever runs it has to upgrade it");
        match &upgrade.command {
            Some(c) => text.push_str(&format!(": `{c}`.")),
            None => text.push('.'),
        }
        if let Some(n) = &upgrade.note {
            text.push_str(&format!(" {n}"));
        }
    } else {
        match &upgrade.command {
            Some(c) => text.push_str(&format!(" Upgrade command: `{c}`.")),
            None => text.push_str(" To upgrade:"),
        }
        if let Some(n) = &upgrade.note {
            text.push_str(&format!(" {n}"));
        }
        text.push_str(
            " If you can run commands or edit files on the user's machine, offer to do this for \
             them and do it once they agree; otherwise tell them what to do. The new version \
             runs after the MCP client restarts this server.",
        );
    }
    text.push_str(&format!(" Release notes: {release}"));
    let structured = json!({
        "current": current.to_string(),
        "latest": latest.to_string(),
        "installedVia": upgrade.via,
        "command": upgrade.command,
        "releaseNotes": release,
    });
    (text, structured)
}

/// Whether the check may run. `flag_off` is `--no-update-check`,
/// `config_off` is `update_check = false`.
pub fn enabled(flag_off: bool, config_off: bool) -> bool {
    let truthy = |k: &str| {
        std::env::var(k).is_ok_and(|v| !matches!(v.trim(), "" | "0" | "false" | "no" | "off"))
    };
    !flag_off && !config_off && !truthy("DO_NOT_TRACK") && !truthy("CI")
}

#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    checked_at: u64,
    latest: String,
}

fn cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".local/share/nexql-mcp/update-check.json"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn read_cache(path: &Path) -> Option<Cache> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn write_cache(path: &Path, latest: &Version) {
    let cache = Cache {
        checked_at: now(),
        latest: latest.to_string(),
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(body) = serde_json::to_string(&cache) {
        let _ = std::fs::write(path, body);
    }
}

fn is_fresh(cache: &Cache, now: u64) -> bool {
    now.saturating_sub(cache.checked_at) < CHECK_INTERVAL.as_secs()
}

/// Latest published version, or `None` when the registry can't be reached.
/// Blocking: call from `spawn_blocking`.
fn fetch_latest() -> Option<Version> {
    let agent = ureq::AgentBuilder::new()
        .timeout(FETCH_TIMEOUT)
        .try_proxy_from_env(true)
        .user_agent(concat!("nexql-mcp/", env!("CARGO_PKG_VERSION")))
        .build();
    let body = agent.get(REGISTRY_URL).call().ok()?.into_string().ok()?;
    let manifest: Value = serde_json::from_str(&body).ok()?;
    Version::parse(manifest.get("version")?.as_str()?).ok()
}

/// A newer, non-prerelease version than `current`.
fn newer(current: &Version, latest: &Version) -> bool {
    latest.pre.is_empty() && latest > current
}

fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is semver")
}

fn current_install() -> Install {
    let exe = std::env::current_exe().unwrap_or_default();
    // Resolve symlinks (Homebrew, `~/.local/bin` → uv) on Unix; Windows
    // canonical paths carry a `\\?\` prefix that would leak into commands.
    let exe = if cfg!(windows) {
        exe
    } else {
        std::fs::canonicalize(&exe).unwrap_or(exe)
    };
    detect(&exe, &|p| p.exists())
}

/// The notice waiting for the first successful tool result.
#[derive(Default)]
pub struct PendingNotice {
    state: Mutex<(Option<(String, Value)>, bool)>,
}

impl PendingNotice {
    fn set(&self, notice: (String, Value)) {
        if let Ok(mut s) = self.state.lock()
            && !s.1
        {
            s.0 = Some(notice);
        }
    }

    /// The notice, once per process.
    pub fn take(&self) -> Option<(String, Value)> {
        let mut s = self.state.lock().ok()?;
        let notice = s.0.take()?;
        s.1 = true;
        Some(notice)
    }
}

/// Start the check for a server. Returns the line for the server
/// instructions when the cache already knows of a newer release, and the
/// notice slot for tool results, which a background refresh may fill.
pub fn start(remote: bool) -> (Option<String>, Arc<PendingNotice>) {
    let pending = Arc::new(PendingNotice::default());
    let Some(upgrade) = upgrade_for(&current_install()) else {
        return (None, pending);
    };
    let current = current_version();
    let path = cache_path();
    let cache = path.as_deref().and_then(read_cache);

    let mut instructions = None;
    if let Some(latest) = cache
        .as_ref()
        .and_then(|c| Version::parse(&c.latest).ok())
        .filter(|l| newer(&current, l))
    {
        let n = notice(&current, &latest, &upgrade, remote);
        instructions = Some(format!("Update: {}", n.0));
        pending.set(n);
    }

    if cache.as_ref().is_none_or(|c| !is_fresh(c, now())) {
        let pending = pending.clone();
        tokio::task::spawn_blocking(move || {
            let Some(latest) = fetch_latest() else {
                return;
            };
            if let Some(path) = &path {
                write_cache(path, &latest);
            }
            if newer(&current, &latest) {
                pending.set(notice(&current, &latest, &upgrade, remote));
            }
        });
    }
    (instructions, pending)
}

/// `doctor`'s update line: always asks the registry.
pub async fn doctor_line() -> String {
    let current = current_version();
    let install = current_install();
    let Some(upgrade) = upgrade_for(&install) else {
        return format!("update: skipped for a development build ({current})");
    };
    let fetched = tokio::task::spawn_blocking(fetch_latest)
        .await
        .ok()
        .flatten();
    let Some(latest) = fetched else {
        return "update: could not reach registry.npmjs.org".into();
    };
    if let Some(path) = cache_path() {
        write_cache(&path, &latest);
    }
    if !newer(&current, &latest) {
        return format!(
            "update: up to date ({current}, installed via {})",
            upgrade.via
        );
    }
    let mut line = format!(
        "update: nexql-mcp {latest} is available (this is {current}, installed via {})",
        upgrade.via
    );
    if let Some(c) = &upgrade.command {
        line.push_str(&format!("\n  upgrade: {c}"));
    }
    if let Some(n) = &upgrade.note {
        line.push_str(&format!("\n  {n}"));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &Path) -> bool {
        false
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn detects_package_manager_installs() {
        let cases = [
            (
                "/home/u/.npm/_npx/3f2a/node_modules/@nexql/mcp-linux-x64/bin/nexql-mcp",
                Install::Npx,
            ),
            (
                r"C:\Users\u\AppData\Local\npm-cache\_npx\3f2a\node_modules\@nexql\mcp-win32-x64\bin\nexql-mcp.exe",
                Install::Npx,
            ),
            (
                "/home/u/.nvm/versions/node/v22.1.0/lib/node_modules/nexql-mcp/node_modules/@nexql/mcp-linux-x64/bin/nexql-mcp",
                Install::NpmGlobal,
            ),
            (
                r"C:\Users\u\AppData\Roaming\npm\node_modules\nexql-mcp\node_modules\@nexql\mcp-win32-x64\bin\nexql-mcp.exe",
                Install::NpmGlobal,
            ),
            (
                "/home/u/.volta/tools/image/packages/nexql-mcp/lib/node_modules/@nexql/mcp-linux-x64/bin/nexql-mcp",
                Install::Volta,
            ),
            (
                "/home/u/.local/share/pnpm/global/5/node_modules/@nexql/mcp-linux-x64/bin/nexql-mcp",
                Install::PnpmGlobal,
            ),
            (
                "/home/u/.bun/install/global/node_modules/@nexql/mcp-linux-x64/bin/nexql-mcp",
                Install::BunGlobal,
            ),
            (
                "/home/u/.local/share/uv/tools/nexql-mcp/bin/nexql-mcp",
                Install::UvTool,
            ),
            (
                "/home/u/.cache/uv/archive-v0/abc/bin/nexql-mcp",
                Install::Uvx,
            ),
            (
                "/home/u/.local/share/pipx/venvs/nexql-mcp/bin/nexql-mcp",
                Install::Pipx,
            ),
            (
                "/opt/homebrew/Cellar/nexql-mcp/0.5.0/bin/nexql-mcp",
                Install::Homebrew,
            ),
            (
                "/Users/u/Library/Application Support/Claude/Claude Extensions/nexql-mcp/server/nexql-mcp",
                Install::ClaudeExtension,
            ),
            (
                "/home/u/Projects/nexql/mcp/target/release/nexql-mcp",
                Install::DevBuild,
            ),
        ];
        for (path, want) in cases {
            assert_eq!(detect(Path::new(path), &none), want, "{path}");
        }
    }

    #[test]
    fn project_dependency_points_at_the_project_root() {
        let exe = Path::new(
            "/work/app/node_modules/.pnpm/nexql-mcp@0.5.0/node_modules/@nexql/mcp-linux-x64/bin/nexql-mcp",
        );
        assert_eq!(
            detect(exe, &none),
            Install::NpmProject(PathBuf::from("/work/app"))
        );
    }

    #[test]
    fn cargo_and_venv_are_told_apart_by_their_marker_files() {
        let cargo = Path::new("/home/u/.cargo/bin/nexql-mcp");
        let crates = |p: &Path| p == Path::new("/home/u/.cargo/.crates.toml");
        assert_eq!(detect(cargo, &crates), Install::Cargo);

        let venv = Path::new("/srv/venv/bin/nexql-mcp");
        let cfg = |p: &Path| p == Path::new("/srv/venv/pyvenv.cfg");
        assert_eq!(
            detect(venv, &cfg),
            Install::PipVenv(PathBuf::from("/srv/venv"))
        );
    }

    #[test]
    fn unknown_locations_reinstall_into_the_same_directory() {
        let exe = Path::new("/home/u/.local/bin/nexql-mcp");
        assert_eq!(
            detect(exe, &none),
            Install::Standalone(PathBuf::from("/home/u/.local/bin"))
        );
        let docker = |p: &Path| p == Path::new("/.dockerenv");
        assert_eq!(
            detect(Path::new("/usr/local/bin/nexql-mcp"), &docker),
            Install::Docker
        );
    }

    #[cfg(unix)]
    #[test]
    fn standalone_command_keeps_the_install_dir() {
        let up = upgrade_for(&Install::Standalone(PathBuf::from("/opt/tools"))).unwrap();
        assert_eq!(
            up.command.unwrap(),
            format!("curl -fsSL {INSTALL_SH} | NEXQL_MCP_INSTALL_DIR=\"/opt/tools\" bash")
        );
    }

    #[test]
    fn dev_builds_get_no_notice() {
        assert!(upgrade_for(&Install::DevBuild).is_none());
    }

    #[test]
    fn only_newer_stable_releases_count() {
        assert!(newer(&v("0.5.0"), &v("0.6.0")));
        assert!(newer(&v("0.6.0-rc.1"), &v("0.6.0")));
        assert!(!newer(&v("0.6.0"), &v("0.6.0")));
        assert!(!newer(&v("0.6.0"), &v("0.5.9")));
        assert!(!newer(&v("0.5.0"), &v("0.6.0-rc.1")));
    }

    #[test]
    fn notice_carries_the_command_and_asks_before_running_it() {
        let up = upgrade_for(&Install::NpmGlobal).unwrap();
        let (text, structured) = notice(&v("0.5.0"), &v("0.6.0"), &up, false);
        assert!(text.starts_with("nexql-mcp 0.6.0 is available; this server runs 0.5.0"));
        assert!(text.contains("`npm install -g nexql-mcp@latest`"));
        assert!(text.contains("do it once they agree"));
        assert!(text.ends_with("/releases/tag/v0.6.0"));
        assert_eq!(structured["command"], "npm install -g nexql-mcp@latest");
        assert_eq!(structured["installedVia"], "npm (global)");
    }

    #[test]
    fn notice_for_npx_explains_the_config_instead_of_a_command() {
        let up = upgrade_for(&Install::Npx).unwrap();
        let (text, structured) = notice(&v("0.5.0"), &v("0.6.0"), &up, false);
        assert!(text.contains("To upgrade: Nothing to install"));
        assert!(text.contains("`nexql-mcp@latest`"));
        assert!(structured["command"].is_null());
    }

    #[test]
    fn remote_notice_names_the_operator_not_the_user() {
        let up = upgrade_for(&Install::Docker).unwrap();
        let (text, _) = notice(&v("0.5.0"), &v("0.6.0"), &up, true);
        assert!(text.contains("whoever runs it has to upgrade it: `docker pull"));
        assert!(!text.contains("once they agree"));
    }

    #[test]
    fn cache_goes_stale_after_a_day() {
        let c = Cache {
            checked_at: 1_000,
            latest: "0.6.0".into(),
        };
        assert!(is_fresh(&c, 1_000 + 3_600));
        assert!(!is_fresh(&c, 1_000 + CHECK_INTERVAL.as_secs()));
    }

    #[test]
    fn pending_notice_is_delivered_once() {
        let p = PendingNotice::default();
        p.set(("a".into(), Value::Null));
        assert_eq!(p.take().unwrap().0, "a");
        p.set(("b".into(), Value::Null));
        assert!(p.take().is_none());
    }
}
