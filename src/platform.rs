//! OS boundaries shared by UI, library jobs and packaging. Never store secrets here.
use std::path::PathBuf;

pub fn data_directory() -> Option<PathBuf> {
    data_path(std::env::consts::OS, |key| {
        std::env::var_os(key).map(PathBuf::from)
    })
}
fn data_path(os: &str, env: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let absolute = |key| env(key).filter(|p| p.is_absolute());
    match os {
        "windows" => absolute("LOCALAPPDATA").map(|p| p.join("MusicCraft")),
        "macos" => absolute("HOME").map(|p| p.join("Library/Application Support/MusicCraft")),
        _ => absolute("XDG_DATA_HOME")
            .or_else(|| absolute("HOME").map(|p| p.join(".local/share")))
            .map(|p| p.join("musiccraft")),
    }
}

/// Use the OS-owned executable, never a binary next to a selected music file.
pub fn curl_command() -> std::process::Command {
    #[cfg(target_os = "windows")]
    let binary = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32/curl.exe");
    #[cfg(not(target_os = "windows"))]
    let binary = PathBuf::from("/usr/bin/curl");
    let mut command = std::process::Command::new(binary);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW for background transfers.
    }
    // Also disables curlrc loading, including credentials or unsafe proxy overrides.
    command.arg("--disable");
    command
}

/// Fonts remain on the user's machine. No proprietary OS fonts are redistributed.
pub fn font_paths(language: &str, all_scripts: bool) -> Vec<(PathBuf, bool)> {
    let mut fonts = Vec::new();
    #[cfg(target_os = "macos")]
    let (latin, cjk, arabic, telugu) = (
        "/System/Library/Fonts/Supplemental/Arial.ttf".into(),
        "/System/Library/Fonts/Hiragino Sans GB.ttc".into(),
        "/System/Library/Fonts/SFArabic.ttf".into(),
        "/System/Library/Fonts/KohinoorTelugu.ttc".into(),
    );
    #[cfg(target_os = "windows")]
    let (latin, cjk, arabic, telugu) = {
        let root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("Fonts");
        (
            root.join("segoeui.ttf"),
            root.join("msyh.ttc"),
            root.join("arial.ttf"),
            root.join("Nirmala.ttf"),
        )
    };
    #[cfg(target_os = "linux")]
    let (latin, cjk, arabic, telugu) = (
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into(),
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".into(),
        "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf".into(),
        "/usr/share/fonts/truetype/noto/NotoSansTelugu-Regular.ttf".into(),
    );
    if language != "ar" {
        fonts.push((latin, true));
    }
    if all_scripts || matches!(language, "ja" | "zh-hans" | "zh-hant") {
        fonts.push((cjk, false));
    }
    if all_scripts || language == "ar" {
        fonts.push((arabic, false));
    }
    if all_scripts || language == "te" {
        fonts.push((telugu, false));
    }
    fonts
}

#[cfg(not(target_os = "macos"))]
mod monitors;
#[cfg(not(target_os = "macos"))]
pub use monitors::areas as monitor_areas;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn data_paths_are_persistent_absolute_and_keep_the_mac_profile() {
        let root = if cfg!(windows) {
            PathBuf::from(r"C:\fixture")
        } else {
            PathBuf::from("/fixture")
        };
        let env = |key: &str| match key {
            "HOME" => Some(root.join("home")),
            "XDG_DATA_HOME" => Some(root.join("data")),
            "LOCALAPPDATA" => Some(root.join("local")),
            _ => None,
        };
        assert_eq!(
            data_path("macos", env).unwrap(),
            root.join("home/Library/Application Support/MusicCraft")
        );
        assert_eq!(
            data_path("linux", env).unwrap(),
            root.join("data/musiccraft")
        );
        assert_eq!(
            data_path("windows", env).unwrap(),
            root.join("local/MusicCraft")
        );
        assert!(data_path("linux", |_| None).is_none());
        assert_eq!(
            data_path("linux", |key| match key {
                "XDG_DATA_HOME" => Some("relative".into()),
                "HOME" => Some(root.join("home")),
                _ => None,
            })
            .unwrap(),
            root.join("home/.local/share/musiccraft")
        );
    }
}
