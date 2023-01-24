#![cfg_attr(not(feature = "std"), no_std)]

use core::f64::consts::PI;
use core::time::Duration;
#[cfg(feature = "std")]
use data_transfer_objects::{BenchmarkData, BenchmarkDataType};
#[cfg(feature = "std")]
use data_transfer_objects::{MotorMonitorParameters, RequestProcessingModel};
#[cfg(feature = "std")]
use log::{debug, error, trace, warn};
use postcard::accumulator::{CobsAccumulator, FeedResult};
#[cfg(feature = "std")]
use postcard::to_allocvec_cobs;
#[cfg(feature = "std")]
use procfs::process::Process;
#[cfg(feature = "std")]
use serde::Deserialize;
#[cfg(feature = "std")]
use std::io::Read;
#[cfg(feature = "std")]
use std::io::Write;
#[cfg(feature = "std")]
use std::net::TcpStream;
#[cfg(feature = "std")]
use std::str::FromStr;
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
pub fn get_now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Could not get epoch seconds")
        .as_secs_f64()
}

#[cfg(feature = "std")]
pub fn get_now_duration() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Could not get epoch seconds")
}

pub fn rpm_to_rad(rpm: f64) -> f64 {
    rpm / 60.0 * PI * 2.0
}

pub fn get_duration_to_end(start_time: Duration, duration: Duration) -> Duration {
    eprintln!(
        "start time: {:?}, now: {:?}, duration: {:?}",
        start_time,
        get_now_duration(),
        duration
    );
    eprintln!("Result: {:?}", start_time - get_now_duration() + duration);
    start_time - get_now_duration() + duration
}

#[cfg(feature = "std")]
pub fn save_benchmark_readings(id: u32, benchmark_data_type: BenchmarkDataType) {
    let me = Process::myself().expect("Could not get process info handle");
    let (cstime, cutime) = me
        .tasks()
        .unwrap()
        .flatten()
        .map(|task| task.stat().unwrap())
        .fold((0, 0), |(stime, utime), task_stat| {
            (stime + task_stat.stime, utime + task_stat.utime)
        });
    let stat = me.stat().expect("Could not get /proc/[pid]/stat info");
    let status = me.status().expect("Could not get /proc/[pid]/status info");
    let benchmark_data = BenchmarkData {
        id,
        time_spent_in_user_mode: stat.utime,
        time_spent_in_kernel_mode: stat.stime,
        children_time_spent_in_user_mode: cutime,
        children_time_spent_in_kernel_mode: cstime,
        peak_resident_set_size: status.vmhwm.expect("Could not get vmhw"),
        peak_virtual_memory_size: status.vmpeak.expect("Could not get vmrss"),
        benchmark_data_type,
    };
    let vec: Vec<u8> =
        to_allocvec_cobs(&benchmark_data).expect("Could not write benchmark data to Vec<u8>");
    let _ = std::io::stdout()
        .write(&vec)
        .expect("Could not write benchmark data bytes to stdout");
    eprintln!("Wrote benchmark data");
}

#[cfg(feature = "std")]
pub fn get_motor_monitor_parameters(arguments: &[String]) -> MotorMonitorParameters {
    MotorMonitorParameters {
        start_time: arguments
            .get(1)
            .expect("Did not receive at least 2 arguments")
            .parse()
            .expect("Could not parse start_time successfully"),
        duration: arguments
            .get(2)
            .expect("Did not receive at least 3 arguments")
            .parse()
            .expect("Could not parse duration successfully"),
        request_processing_model: RequestProcessingModel::from_str(
            arguments
                .get(3)
                .expect("Did not receive at least 4 arguments"),
        )
        .expect("Could not parse Request Processing Model successfully"),
        number_of_tcp_motor_groups: arguments
            .get(4)
            .expect("Did not receive at least 5 arguments")
            .parse()
            .expect("Could not parse number_of_motor_groups successfully"),
        number_of_i2c_motor_groups: arguments
            .get(5)
            .expect("Did not receive at least 5 arguments")
            .parse()
            .expect("Could not parse number_of_motor_groups successfully"),
        window_size: arguments
            .get(6)
            .expect("Did not receive at least 6 arguments")
            .parse()
            .expect("Could not parse window_size successfully"),
        sensor_listen_address: arguments
            .get(7)
            .expect("Did not receive at least 7 arguments")
            .parse()
            .expect("Could not parse sensor listen address successfully"),
        motor_monitor_listen_address: arguments
            .get(8)
            .expect("Did not receive at least 8 arguments")
            .parse()
            .expect("Could not parse motor monitor listen address successfully"),
        sampling_interval: arguments
            .get(9)
            .expect("Did not receive at least 9 arguments")
            .parse()
            .expect("Could not parse sampling_interval successfully"),
        thread_pool_size: arguments
            .get(10)
            .expect("Did not receive at least 10 arguments")
            .parse()
            .expect("Could not parse thread_pool_size successfully"),
    }
}
