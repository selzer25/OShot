//! Recent captures, kept as PNG files with small thumbnails for the menu.

use std::path::{Path, PathBuf};

use oshot_core::naming::LocalTime;

#[derive(Clone, Debug, PartialEq)]
pub struct RecentItem {
    pub path: PathBuf,
    pub thumb: PathBuf,
    pub created_ms: u64,
}

impl RecentItem {
    pub fn title(&self) -> String {
        let t = LocalTime::from_unix((self.created_ms / 1000) as i64);
        format!(
            "{:02}:{:02}:{:02} · {}-{:02}-{:02}",
            t.hour, t.minute, t.second, t.year, t.month, t.day
        )
    }
}

/// Newest first.
pub fn list(dir: &Path) -> Vec<RecentItem> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items: Vec<RecentItem> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?.to_string();
            let stem = name.strip_suffix(".png")?;
            if stem.ends_with(".thumb") {
                return None;
            }
            let created_ms = stem.parse().ok()?;
            Some(RecentItem {
                thumb: dir.join(format!("{stem}.thumb.png")),
                path,
                created_ms,
            })
        })
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.created_ms));
    items
}

/// Stores a capture and drops the oldest beyond `limit`.
pub fn add(dir: &Path, png: &[u8], thumb_png: &[u8], limit: u32) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    while dir.join(format!("{ms}.png")).exists() {
        ms += 1;
    }
    let path = dir.join(format!("{ms}.png"));
    std::fs::write(&path, png)?;
    std::fs::write(dir.join(format!("{ms}.thumb.png")), thumb_png)?;
    for old in list(dir).into_iter().skip(limit.max(1) as usize) {
        let _ = std::fs::remove_file(&old.path);
        let _ = std::fs::remove_file(&old.thumb);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_newest_within_limit() {
        let dir = std::env::temp_dir().join(format!("oshot-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for i in 0..4u8 {
            add(&dir, &[i], &[i], 3).unwrap();
        }
        let items = list(&dir);
        assert_eq!(items.len(), 3);
        assert_eq!(std::fs::read(&items[0].path).unwrap(), vec![3]);
        assert!(items[0].thumb.exists());
        assert!(!items[0].title().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
