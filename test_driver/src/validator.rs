use crate::Args;
use data_transfer_objects::{Alert, MotorFailure};
use log::{error, info};
use rand::prelude::IteratorRandom;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use std::cmp::{max, min};
use std::fs;
use std::io::BufRead;
use std::ops::Shl;
use std::time::Duration;

pub(crate) fn validate_alerts(args: &Args, start_time: Duration, alerts: &[Alert]) {
    info!("Validating {} alerts", alerts.len());
    let expected_alerts = get_expected_alerts(args, start_time);
    info!("Expecting {} alerts", expected_alerts.len());
    let mut erroneous_alerts: Vec<(String, &Alert)> = expected_alerts
        .iter()
        .filter(|expected_alert| {
            !alerts.iter().any(|alert| {
                alert_equals(
                    Duration::from_secs(args.window_size_seconds),
                    expected_alert,
                    alert,
                )
            })
        })
        .map(|alert| ("Expected".to_string(), alert))
        .collect();
    let not_expected_alerts: Vec<(String, &Alert)> = alerts
        .iter()
        .filter(|alert| {
            !expected_alerts.iter().any(|expected_alert| {
                alert_equals(
                    Duration::from_secs(args.window_size_seconds),
                    expected_alert,
                    alert,
                )
            })
        })
        .map(|alert| ("Received".to_string(), alert))
        .collect();
    erroneous_alerts.append(&mut not_expected_alerts.clone());
    erroneous_alerts.sort_by_key(|alert| alert.1.time.round() as u64);
    error!("{} errors in total", erroneous_alerts.len());
    for erroneous_alert in erroneous_alerts {
        error!("{}: {:?}", erroneous_alert.0, erroneous_alert.1);
    }
}

fn alert_equals(validation_window: Duration, expected_alert: &Alert, alert: &Alert) -> bool {
    expected_alert.failure == alert.failure
        && expected_alert.motor_id == alert.motor_id
        && (expected_alert.time - alert.time).abs() <= validation_window.as_secs_f64()
}

pub(crate) fn get_expected_alerts(args: &Args, start_time: Duration) -> Vec<Alert> {
    let window_size = args.window_size_seconds * 1000 / args.sampling_interval_ms as u64;
    let mut alerts: Vec<Alert> = Vec::new();
    for i in 0..args.motor_groups_i2c as u16 + args.motor_groups_tcp {
        let mut buffer: [Vec<(Duration, f32)>; 4] =
            [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for j in 0..4 {
            let seed: u32 = (i as u32).shl(2) + j as u32;
            let mut rng = SmallRng::seed_from_u64(seed as u64);
            let mut time = start_time;
            while time < start_time + Duration::from_secs(args.duration) {
                let sensor_reading = get_sensor_reading(&mut rng, j);
                buffer[j as usize].push((time, sensor_reading));
                time += Duration::from_millis(args.sampling_interval_ms as u64);
            }
        }
        alerts.append(&mut get_motor_alerts(i, buffer, window_size, start_time));
    }
    alerts.sort_by_key(|alert| alert.time.round() as u64);
    alerts
}

fn get_motor_alerts(
    motor_id: u16,
    buffer: [Vec<(Duration, f32)>; 4],
    window_size: u64,
    start_time: Duration,
) -> Vec<Alert> {
    let mut alerts = Vec::new();
    let mut sensor_replacing_time = start_time;
    for i in 0..buffer[0].len() {
        let air_temperature = get_average_value(i, window_size, &buffer[0]);
        let process_temperature = get_average_value(i, window_size, &buffer[1]);
        let rotational_speed = get_average_value(i, window_size, &buffer[2]);
        let torque = get_average_value(i, window_size, &buffer[3]);
        let rotational_speed_in_rad = utils::rpm_to_rad(rotational_speed);
        let time = buffer[0][i].0;
        let age = time - sensor_replacing_time;
        if (air_temperature - process_temperature).abs() < 8.6 && rotational_speed < 1380.0 {
            alerts.push(Alert {
                time: (time - start_time).as_secs_f64(),
                motor_id,
                failure: MotorFailure::HeatDissipationFailure,
            });
            sensor_replacing_time = time;
        } else if torque * rotational_speed_in_rad > 9000.0
            || torque * rotational_speed_in_rad < 3500.0
        {
            alerts.push(Alert {
                time: (time - start_time).as_secs_f64(),
                motor_id,
                failure: MotorFailure::PowerFailure,
            });
            sensor_replacing_time = time;
        } else if age.as_secs_f64() * torque.round() > 10000_f64 {
            alerts.push(Alert {
                time: (time - start_time).as_secs_f64(),
                motor_id,
                failure: MotorFailure::OverstrainFailure,
            });
            sensor_replacing_time = time;
        }
    }
    alerts
}

fn get_average_value(position: usize, window_size: u64, buffer: &[(Duration, f32)]) -> f64 {
    let mut accumulator: f64 = 0.0;
    for i in (max(0, position as i32 - window_size as i32) as usize)..position {
        accumulator += buffer[i + 1].1 as f64;
    }
    accumulator / min(position as i32, window_size as i32) as f64
}

fn get_sensor_reading(rng: &mut SmallRng, j: i32) -> f32 {
    fs::read(format!("resources/{j}.txt"))
        .expect("Failure reading sensor data")
        .lines()
        .choose_stable(rng)
        .expect("Data file iterator is empty")
        .expect("Error reading from data file iterator")
        .parse()
        .expect("Error parsing data fileline")
}
