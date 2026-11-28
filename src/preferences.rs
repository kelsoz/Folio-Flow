//! Per-user preferences, independent of the portable executable location.
use std::{io, path::PathBuf};

pub fn directory() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|root| root.join("Folio Flow"))
}

pub fn load_light() -> bool {
    directory()
        .map(|p| p.join("theme.txt"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|value| value.trim() == "light")
}

pub fn save_light(light: bool) -> io::Result<()> {
    let path = directory()
        .map(|p| p.join("theme.txt"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No user config directory"))?;
    write_theme(&path, light)
}

fn write_theme(path: &std::path::Path, light: bool) -> io::Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, if light { "light\n" } else { "dark\n" })
}

pub fn load_remember() -> bool {
    directory()
        .and_then(|p| std::fs::read_to_string(p.join("remember.txt")).ok())
        .is_none_or(|s| s.trim() != "off")
}
pub fn save_remember(remember: bool) -> io::Result<()> {
    let dir = directory().ok_or_else(|| io::Error::other("No user config directory"))?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("remember.txt"),
        if remember { "on" } else { "off" },
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn theme_survives_reopening_and_can_be_replaced() {
        let root = std::env::temp_dir().join(format!("folio-theme-{}", std::process::id()));
        let path = root.join("theme.txt");
        super::write_theme(&path, true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "light");
        super::write_theme(&path, false).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "dark");
        std::fs::remove_dir_all(root).unwrap();
    }
}
