//! Auto-update: GitHub Releases check on startup, download, self-replace.
//!
//! The user configures the source repo in Preferences → Обновление
//! (default `https://github.com/BagmanovIlnar/PL-SQL-IDE`). On boot the app
//! asks `<api>/releases/latest` once; when the tag is newer than the running
//! version the UI offers an update, and on agreement downloads the platform
//! asset and atomically replaces the running executable.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// Default update source (user-overridable in Preferences, survives moves).
pub const DEFAULT_UPDATE_URL: &str = "https://github.com/BagmanovIlnar/PL-SQL-IDE";

/// Newer release found by the startup check.
#[derive(Debug, Clone)]
pub struct UpdateOffer {
    /// Release tag without the leading `v`, e.g. `0.0.4`.
    pub version: String,
    /// `browser_download_url` of the asset for the current platform.
    pub asset_url: String,
}

/// Asks GitHub for the latest release; `None` on any error or when the
/// running version is up to date (offline startup stays silent).
pub fn check(base_url: &str) -> Option<UpdateOffer> {
    let api = format!(
        "https://api.github.com/repos/{}/releases/latest",
        repo_slug(base_url)?
    );
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build();
    let json: serde_json::Value = agent
        .get(&api)
        .set("Accept", "application/vnd.github+json")
        .call()
        .ok()
        .and_then(|r| serde_json::from_reader(r.into_reader()).ok())?;
    let tag = json["tag_name"].as_str()?;
    let version = tag.trim().trim_start_matches('v').to_string();
    if !newer(&version, env!("CARGO_PKG_VERSION")) {
        return None;
    }
    let assets = json["assets"].as_array()?;
    let asset = assets
        .iter()
        .find(|a| a["name"].as_str().is_some_and(is_platform_asset))?;
    Some(UpdateOffer {
        version,
        asset_url: asset["browser_download_url"].as_str()?.to_string(),
    })
}

/// Downloads the platform asset and replaces the running executable.
/// The old binary is kept next to the new one as `*.bak` for rollback.
pub fn install(offer: &UpdateOffer) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no exe dir")?.to_path_buf();
    // ponytail: staging next to the exe so rename stays on one volume.
    let tmp = dir.join(format!(".update-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

    let name = offer.asset_url.rsplit('/').next().unwrap_or("download");
    let archive = tmp.join(name);
    download(&offer.asset_url, &archive)?;
    let new_exe = match platform() {
        Platform::Windows => archive, // plain .exe asset, no unpacking
        Platform::Unix => {
            let (tool, args) = if name.ends_with(".tar.gz") {
                (
                    "tar",
                    vec![
                        "-xzf".into(),
                        archive.display().to_string(),
                        "-C".into(),
                        tmp.display().to_string(),
                    ],
                )
            } else {
                (
                    "unzip",
                    vec![
                        "-oq".into(),
                        archive.display().to_string(),
                        "-d".into(),
                        tmp.display().to_string(),
                    ],
                )
            };
            // ponytail: system unpacker, assets come from our own CI (trusted).
            let status = Command::new(tool)
                .args(&args)
                .status()
                .map_err(|e| format!("{tool}: {e}"))?;
            if !status.success() {
                return Err(format!("{tool} exited with {status}"));
            }
            find_binary(&tmp).ok_or("binary not found in archive")?
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&new_exe, fs::Permissions::from_mode(0o755));
    }

    let bak = dir.join(format!(
        "{}.bak",
        exe.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("plsqlide")
    ));
    fs::rename(&exe, &bak).map_err(|e| format!("rename running exe: {e}"))?;
    if let Err(e) = fs::rename(&new_exe, &exe) {
        let _ = fs::rename(&bak, &exe); // put the old binary back
        return Err(format!("install: {e}"));
    }
    let _ = fs::remove_dir_all(&tmp);
    Ok(())
}

/// Launches the freshly installed binary and quits; the OS window closes.
pub fn restart() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe).spawn();
    }
    std::process::exit(0);
}

/// `https://github.com/OWNER/REPO[...]` → `OWNER/REPO`.
fn repo_slug(base_url: &str) -> Option<String> {
    let rest = base_url.trim().rsplit("github.com/").next()?;
    let mut parts = rest.trim_end_matches('/').split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

/// Numeric dotted compare: `newer("0.0.10", "0.0.9") == true`.
fn newer(remote: &str, local: &str) -> bool {
    fn parse(v: &str) -> Vec<u64> {
        v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
    }
    parse(remote) > parse(local)
}

#[derive(Clone, Copy, PartialEq)]
enum Platform {
    Windows,
    Unix,
}

fn platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Unix
    }
}

/// Release asset matching the platform we were built for.
fn is_platform_asset(name: &str) -> bool {
    match platform() {
        Platform::Windows => name.ends_with("-windows-x86_64.exe"),
        Platform::Unix => {
            if cfg!(target_os = "macos") {
                name.contains("-macos-aarch64.zip")
            } else {
                name.contains("-linux-x86_64.tar.gz")
            }
        }
    }
}

fn download(url: &str, to: &PathBuf) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .build();
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    let mut file = fs::File::create(to).map_err(|e| e.to_string())?;
    let mut reader = response.into_reader();
    std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
    file.flush().ok();
    Ok(())
}

/// Walks the unpacked tree for the app binary (`plsqlide` inside the bundle).
fn find_binary(root: &PathBuf) -> Option<PathBuf> {
    fn walk(dir: &PathBuf) -> Option<PathBuf> {
        for entry in fs::read_dir(dir).ok()? {
            let path = entry.ok()?.path();
            if path.is_dir() {
                if let Some(hit) = walk(&path) {
                    return Some(hit);
                }
            } else if path.file_name().and_then(|n| n.to_str()) == Some("plsqlide") {
                return Some(path);
            }
        }
        None
    }
    walk(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(newer("0.0.4", "0.0.3"));
        assert!(newer("0.1.0", "0.0.9"));
        assert!(newer("1.0", "0.9.9"));
        assert!(!newer("0.0.3", "0.0.3"));
        assert!(!newer("0.0.2", "0.0.3"));
    }

    #[test]
    fn slug_from_repo_url() {
        assert_eq!(
            repo_slug(DEFAULT_UPDATE_URL).as_deref(),
            Some("BagmanovIlnar/PL-SQL-IDE")
        );
        assert_eq!(
            repo_slug("https://github.com/Owner/Repo/").as_deref(),
            Some("Owner/Repo")
        );
        assert_eq!(repo_slug("https://example.com/x"), None);
    }
}
