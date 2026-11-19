//! The computer's clock as its owner reads it: the hour of the day where the computer is.

/// The hour of the day on the computer's clock, in its time zone: 0 to 24, with the minutes as a
/// fraction (14.5 is half past two in the afternoon). `None` where it cannot be read.
pub fn local_hour() -> Option<f64> {
    imp::local_hour()
}

/// The date and time on the computer's clock, in its time zone: year, month (1-12), day, hour,
/// minute, second. `None` where it cannot be read.
pub fn local_time() -> Option<[i32; 6]> {
    imp::local_time()
}

#[cfg(unix)]
mod imp {
    pub fn local_hour() -> Option<f64> {
        let [.., hour, minute, second] = local_time()?;
        Some(hour as f64 + minute as f64 / 60.0 + second as f64 / 3600.0)
    }

    pub fn local_time() -> Option<[i32; 6]> {
        // SAFETY: `time` with a null pointer only returns the time; `localtime_r` writes into the
        // `tm` given and keeps nothing (the reentrant one: no shared buffer).
        unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&now, &mut tm).is_null() {
                return None;
            }
            Some([tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec])
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    pub fn local_hour() -> Option<f64> {
        let now = js_sys::Date::new_0();
        Some(now.get_hours() as f64 + now.get_minutes() as f64 / 60.0 + now.get_seconds() as f64 / 3600.0)
    }

    pub fn local_time() -> Option<[i32; 6]> {
        let now = js_sys::Date::new_0();
        Some([now.get_full_year() as i32, now.get_month() as i32 + 1, now.get_date() as i32, now.get_hours() as i32, now.get_minutes() as i32, now.get_seconds() as i32])
    }
}

#[cfg(not(any(unix, target_arch = "wasm32")))]
mod imp {
    pub fn local_hour() -> Option<f64> {
        None
    }

    pub fn local_time() -> Option<[i32; 6]> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_hour_is_one_of_the_day() {
        let hour = super::local_hour().expect("the clock reads on this system");
        assert!((0.0..24.0).contains(&hour), "{hour}");
        let [year, month, day, ..] = super::local_time().unwrap();
        assert!(year >= 2024 && (1..=12).contains(&month) && (1..=31).contains(&day));
    }
}
