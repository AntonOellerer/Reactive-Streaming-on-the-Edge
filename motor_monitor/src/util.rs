use std::f64::consts::PI;
use std::time::{SystemTime, UNIX_EPOCH};

use libc::time_t;

pub fn get_now() -> time_t {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Could not get epoch seconds")
        .as_secs()
        .try_into()
        .expect("Could not convert now start to time_t")
}

pub fn rpm_to_rad(rpm: f64) -> f64 {
    rpm / 60.0 * PI * 2.0
}
