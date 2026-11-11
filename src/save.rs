//! Optional copy under ~/Pictures/Screenshots.

use anyhow::Result;
use chrono::Local;
use std::fs;
use std::path::{Path, PathBuf};

pub fn default_save_dir() -> PathBuf {
    let pictures = dirs::picture_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Pictures")
    });
    pictures.join("Screenshots")
}

pub fn resolve_dir(override_dir: Option<&Path>) -> PathBuf {
    override_dir
        .map(Path::to_path_buf)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(default_save_dir)
}

pub fn filename_for(stamp: chrono::DateTime<Local>) -> String {
    stamp
        .format("Screenshot %Y-%m-%d at %H.%M.%S.png")
        .to_string()
}

/// Pick a path that does not already exist. A burst of shots in the same
/// second grows a numeric suffix instead of overwriting.
pub fn unique_path(dir: &Path, stamp: chrono::DateTime<Local>) -> PathBuf {
    let base = filename_for(stamp);
    let candidate = dir.join(&base);
    if !candidate.exists() {
        return candidate;
    }
    let stem = base.trim_end_matches(".png");
    for n in 2..1000 {
        let path = dir.join(format!("{stem}-{n}.png"));
        if !path.exists() {
            return path;
        }
    }
    dir.join(format!("{stem}-{}.png", std::process::id()))
}

pub fn write_png(dir: &Path, bytes: &[u8]) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = unique_path(dir, Local::now());
    fs::write(&path, bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn filename_uses_the_macos_style_stamp() {
        let stamp = Local.with_ymd_and_hms(2026, 11, 10, 23, 45, 1).unwrap();
        assert_eq!(filename_for(stamp), "Screenshot 2026-11-10 at 23.45.01.png");
    }

    #[test]
    fn unique_path_skips_existing_files() {
        let dir = std::env::temp_dir().join(format!("stillshot-names-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let stamp = Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        let first = dir.join(filename_for(stamp));
        fs::write(&first, b"x").unwrap();
        let next = unique_path(&dir, stamp);
        assert_ne!(next, first);
        assert!(next.file_name().unwrap().to_string_lossy().contains("-2"));
        let _ = fs::remove_dir_all(&dir);
    }
}
