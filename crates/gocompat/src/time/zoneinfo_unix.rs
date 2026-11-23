use super::zoneinfo::{Location, LocationData};
use super::zoneinfo_read::load_location_sources;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::sync::OnceLock;

pub const PLATFORM_ZONE_SOURCES: [&str; 4] = [
    "/usr/share/zoneinfo/",
    "/usr/share/lib/zoneinfo/",
    "/usr/lib/locale/TZ/",
    "/etc/zoneinfo",
];

static LOCAL: OnceLock<LocationData> = OnceLock::new();

pub(crate) fn local_data() -> &'static LocationData {
    LOCAL.get_or_init(|| {
        let tz = std::env::var_os("TZ");
        let platform: Vec<&OsStr> = PLATFORM_ZONE_SOURCES.iter().map(OsStr::new).collect();
        init_local(tz.as_deref(), OsStr::new("/etc"), &platform)
    })
}

pub fn set_local(loc: &Location) -> bool {
    if LOCAL.get().is_some() {
        return false;
    }
    let data = loc.data().clone();
    LOCAL.set(data).is_ok()
}

pub fn local_location_with(tz: Option<&OsStr>, etc_dir: &OsStr, platform: &[&OsStr]) -> Location {
    Location::from_data(init_local(tz, etc_dir, platform))
}

fn init_local(tz: Option<&OsStr>, etc_dir: &OsStr, platform: &[&OsStr]) -> LocationData {
    match tz {
        None => {
            if let Ok(mut z) = load_location_sources(OsStr::new("localtime"), &[etc_dir]) {
                z.name = "Local".to_string();
                return z;
            }
        }
        Some(tz) if !tz.is_empty() => {
            let mut tz = tz.as_bytes();
            if tz[0] == b':' {
                tz = &tz[1..];
            }
            if tz.first() == Some(&b'/') {
                if let Ok(mut z) = load_location_sources(OsStr::from_bytes(tz), &[OsStr::new("")]) {
                    z.name = if tz == b"/etc/localtime" {
                        "Local".to_string()
                    } else {
                        String::from_utf8_lossy(tz).into_owned()
                    };
                    return z;
                }
            } else if !tz.is_empty()
                && tz != b"UTC"
                && let Ok(z) = load_location_sources(OsStr::from_bytes(tz), platform)
            {
                return z;
            }
        }
        Some(_) => {}
    }
    LocationData::empty("UTC")
}
