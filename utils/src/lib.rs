use libc::time_t;
use log::debug;
use postcard::accumulator::{CobsAccumulator, FeedResult};
use serde::Deserialize;
use std::f64::consts::PI;
use std::io::Read;
use std::net::TcpStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn get_object<T>(stream: &mut TcpStream) -> Option<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut raw_buf = [0u8; 32];
    let mut cobs_buf: CobsAccumulator<256> = CobsAccumulator::new();
    let mut alert: Option<T> = None;
    debug!("Reading from stream");
    while let Ok(ct) = stream.read(&mut raw_buf) {
        debug!("Read into buffer: {}", ct);
        // Finished reading input
        if ct == 0 {
            break;
        }
        let mut window = &raw_buf[..ct];
        while alert.is_none() && !window.is_empty() {
            debug!("Reading into accumulator");
            window = match cobs_buf.feed::<T>(window) {
                FeedResult::Consumed => {
                    debug!("Consumed buffer");
                    break;
                }
                FeedResult::OverFull(new_wind) => {
                    debug!("Overfull");
                    new_wind
                }
                FeedResult::DeserError(new_wind) => {
                    debug!("Deserialization error");
                    new_wind
                }
                FeedResult::Success { data, remaining } => {
                    debug!("Deserialized object");
                    alert = Some(data);
                    remaining
                }
            };
            debug!("Read into accumulator");
        }
        debug!("Read full window");
        if alert.is_some() {
            return alert;
        }
    }
    debug!("Read");
    alert
}

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

pub fn get_end_duration(start_time: time_t, duration: u64) -> u64 {
    (start_time - get_now() + duration as i64) as u64
}

pub fn get_sleep_duration(start_time: time_t, duration: u64) -> Duration {
    Duration::from_secs(get_end_duration(start_time, duration))
}
