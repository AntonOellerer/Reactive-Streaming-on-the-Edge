#![cfg_attr(not(feature = "std"), no_std)]

use core::f64::consts::PI;
use core::time::Duration;
#[cfg(feature = "std")]
use std::io::Read;

use libc::time_t;
#[cfg(feature = "std")]
use log::{debug, error, trace, warn};
use postcard::accumulator::{CobsAccumulator, FeedResult};
#[cfg(feature = "std")]
use serde::Deserialize;

#[cfg(feature = "std")]
use std::net::TcpStream;
#[cfg(feature = "std")]
use std::time::SystemTime;
#[cfg(feature = "std")]
use std::time::UNIX_EPOCH;

#[cfg(feature = "std")]
//todo find way to return error object
pub fn read_object<T>(stream: &mut TcpStream) -> Option<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut raw_buf = [0u8; 1];
    let mut cobs_buf: CobsAccumulator<256> = CobsAccumulator::new();
    let mut alert: Option<T> = None;
    trace!("Reading from stream");
    while let Ok(ct) = stream.read(&mut raw_buf) {
        trace!("Read into buffer: {}", ct);
        // Finished reading input
        if ct == 0 {
            break;
        }
        let mut window = &raw_buf[..ct];
        while alert.is_none() && !window.is_empty() {
            trace!("Reading into accumulator");
            window = match cobs_buf.feed::<T>(window) {
                FeedResult::Consumed => {
                    debug!("Consumed buffer");
                    break;
                }
                FeedResult::OverFull(new_wind) => {
                    error!("Overfull");
                    new_wind
                }
                FeedResult::DeserError(new_wind) => {
                    error!("Deserialization error");
                    new_wind
                }
                FeedResult::Success { data, remaining } => {
                    debug!("Deserialized object");
                    alert = Some(data);
                    if !remaining.is_empty() {
                        warn!("Remaining size: {}", remaining.len());
                    }
                    remaining
                }
            };
            trace!("Read into accumulator");
        }
        trace!("Read full window");
        if alert.is_some() {
            return alert;
        }
    }
    trace!("Read");
    alert
}

#[cfg(feature = "std")]
pub fn get_now() -> time_t {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Could not get epoch seconds")
        .as_millis()
        .try_into()
        .expect("Could not convert now start to time_t")
}

pub fn rpm_to_rad(rpm: f64) -> f64 {
    rpm / 60.0 * PI * 2.0
}

pub fn get_secs_to_end(start_time: time_t, duration: u32) -> u32 {
    (start_time - get_now() + duration as time_t) as u32
}

pub fn get_duration_to_end(start_time: time_t, duration: u32) -> Duration {
    Duration::from_secs(get_secs_to_end(start_time, duration) as u64)
}
