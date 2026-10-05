//! XDG base directories: `$XDG_DATA_HOME` and `$XDG_CONFIG_HOME`, or the spec's defaults under the
//! home folder. An empty or relative value is invalid by the spec and ignored.

use std::path::{Path, PathBuf};

/// `$XDG_DATA_HOME`, default `~/.local/share`.
pub fn data_home(home: &Path) -> PathBuf {
    from_env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"))
}

/// `$XDG_CONFIG_HOME`, default `~/.config`.
pub fn config_home(home: &Path) -> PathBuf {
    from_env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"))
}

fn from_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}
