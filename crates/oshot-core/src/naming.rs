//! File names for saved captures.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

#[repr(C)]
struct Tm {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
    tm_gmtoff: i64,
    tm_zone: *mut i8,
}

unsafe extern "C" {
    fn localtime_r(time: *const i64, out: *mut Tm) -> *mut Tm;
}

impl LocalTime {
    pub fn from_unix(secs: i64) -> LocalTime {
        let mut tm = Tm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 1,
            tm_mon: 0,
            tm_year: 70,
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
            tm_gmtoff: 0,
            tm_zone: std::ptr::null_mut(),
        };
        // SAFETY: both pointers are valid for the call; localtime_r is the
        // thread-safe variant and only writes into `tm`.
        unsafe { localtime_r(&secs, &mut tm) };
        LocalTime {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
        }
    }

    pub fn now() -> LocalTime {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        LocalTime::from_unix(secs)
    }
}

pub const DEFAULT_TEMPLATE: &str = "OShot {date} at {time}";

/// Expands `{date}` (2026-11-08), `{time}` (15.04.12), `{year}`, `{month}`,
/// `{day}`, `{hour}`, `{minute}` and `{second}`. Path separators are removed
/// so a template cannot escape the save folder.
pub fn expand_template(template: &str, t: LocalTime) -> String {
    let name = template
        .replace("{date}", &format!("{:04}-{:02}-{:02}", t.year, t.month, t.day))
        .replace("{time}", &format!("{:02}.{:02}.{:02}", t.hour, t.minute, t.second))
        .replace("{year}", &format!("{:04}", t.year))
        .replace("{month}", &format!("{:02}", t.month))
        .replace("{day}", &format!("{:02}", t.day))
        .replace("{hour}", &format!("{:02}", t.hour))
        .replace("{minute}", &format!("{:02}", t.minute))
        .replace("{second}", &format!("{:02}", t.second));
    let cleaned: String = name
        .chars()
        .map(|c| if c == '/' || c == ':' { '-' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        "OShot".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `dir/name.ext`, adding " 2", " 3"… if that file already exists.
pub fn unique_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{name}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{name} {n}.{ext}")))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: LocalTime = LocalTime {
        year: 2026,
        month: 11,
        day: 8,
        hour: 15,
        minute: 4,
        second: 9,
    };

    #[test]
    fn expands_default_template() {
        assert_eq!(expand_template(DEFAULT_TEMPLATE, T), "OShot 2026-11-08 at 15.04.09");
        assert_eq!(expand_template("{year}/{month}:{day}", T), "2026-11-08");
        assert_eq!(expand_template("..", T), "OShot");
    }

    #[test]
    fn unique_path_counts_up() {
        let dir = std::env::temp_dir().join(format!("oshot-naming-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = unique_path(&dir, "shot", "png");
        std::fs::write(&a, b"x").unwrap();
        let b = unique_path(&dir, "shot", "png");
        assert_eq!(b.file_name().unwrap(), "shot 2.png");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn local_time_is_sane() {
        let t = LocalTime::now();
        assert!(t.year >= 2024 && (1..=12).contains(&t.month));
    }
}
