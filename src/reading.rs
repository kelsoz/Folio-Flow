//! A bounded, local reading-position store. PDFs are never modified.
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub page: u32,
    pub top: f32,
    pub zoom: f32,
    pub fit: u8,
    pub double: bool,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    bytes: u64,
    modified: u64,
    position: Position,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    entries: Vec<Entry>,
}

fn identity(path: &Path) -> Option<(PathBuf, u64, u64)> {
    let path = path.canonicalize().ok()?;
    let meta = path.metadata().ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos()
        .try_into()
        .ok()?;
    Some((path, meta.len(), modified))
}

impl Store {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn get(&self, path: &Path) -> Option<Position> {
        let (path, bytes, modified) = identity(path)?;
        let p = &self
            .entries
            .iter()
            .find(|e| e.path == path && e.bytes == bytes && e.modified == modified)?
            .position;
        (p.top.is_finite() && p.zoom.is_finite() && p.zoom > 0.0 && p.fit <= 2).then(|| p.clone())
    }

    pub fn set(&mut self, path: &Path, position: Position) -> bool {
        let Some((path, bytes, modified)) = identity(path) else {
            return false;
        };
        if self.entries.last().is_some_and(|e| {
            e.path == path && e.bytes == bytes && e.modified == modified && e.position == position
        }) {
            return false;
        }
        self.entries.retain(|e| e.path != path);
        self.entries.push(Entry {
            path,
            bytes,
            modified,
            position,
        });
        if self.entries.len() > 100 {
            self.entries.remove(0);
        }
        true
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        let temp = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&temp, bytes)?;
        std::fs::rename(temp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn positions_roundtrip_and_replaced_files_do_not_restore() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../TEMP/reading-test");
        std::fs::create_dir_all(&root).unwrap();
        let pdf = root.join("中文 sample.pdf");
        std::fs::write(&pdf, b"sample").unwrap();
        let record = root.join("positions.json");
        let p = Position {
            page: 4,
            top: 123.5,
            zoom: 1.25,
            fit: 0,
            double: true,
        };
        let mut store = Store::default();
        assert!(store.set(&pdf, p.clone()));
        assert!(!store.set(&pdf, p.clone()));
        store.save(&record).unwrap();
        store.save(&record).unwrap();
        let loaded = Store::load(&record);
        assert_eq!(loaded.get(&pdf), Some(p));
        std::fs::write(&pdf, b"replacement").unwrap();
        assert_eq!(loaded.get(&pdf), None);
        std::fs::write(&record, b"broken JSON").unwrap();
        assert!(Store::load(&record).entries.is_empty());
    }
}
