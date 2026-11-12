use std::path::PathBuf;

const FILE: &str = "ytm-hybrid.log";

/// The file the running YTM Hybrid appends its log to, `ytm-hybrid.log` under the platform's
/// state folder, or the cache folder where the platform has none. `None` when neither is known.
pub fn log_file() -> Option<PathBuf> {
    let root = dirs::state_dir().or_else(dirs::cache_dir)?;

    Some(root.join("ytm-hybrid").join(FILE))
}
