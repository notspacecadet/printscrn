use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static LOG_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init() {
    let path = crate::config::config_dir().join("stillshot.log");
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // Keep the log from growing without bound across months of tray residency.
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > 512 * 1024 {
            let _ = fs::rename(&path, path.with_extension("log.1"));
        }
    }
    *LOG_FILE.lock().unwrap() = Some(path);
    log("stillshot started");
}

pub fn log(message: impl AsRef<str>) {
    let line = format!(
        "[{}] {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        message.as_ref()
    );
    eprint!("{line}");
    if let Some(path) = LOG_FILE.lock().unwrap().as_ref() {
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(line.as_bytes());
        }
    }
}
