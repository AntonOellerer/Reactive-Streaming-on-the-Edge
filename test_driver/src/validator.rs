use crate::Args;
use data_transfer_objects::Alert;
use log::{debug, error, info, trace};
use rand::prelude::IteratorRandom;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use std::cmp::{max, min};
use std::fs;
use std::io::BufRead;
use std::ops::Shl;
use std::time::Duration;

pub(crate) fn validate_alerts(args: &Args, start_time: Duration, alerts: &[Alert]) -> usize {
    info!("Validating {} alerts", alerts.len());
    let expected_alerts = get_expected_alerts(args, start_time);
    info!("Expecting {} alerts", expected_alerts.len());
    let erroneous_alerts = get_alert_failures(
        alerts,
        &expected_alerts,
        Duration::from_millis(args.window_size_ms),
    );
    error!("{} errors in total", erroneous_alerts.len());
    for erroneous_alert in erroneous_alerts.iter() {
        error!("{}: {:?}", erroneous_alert.0, erroneous_alert.1);
    }
    erroneous_alerts.len()
}

pub(crate) fn get_expected_alerts(args: &Args, start_time: Duration) -> Vec<Alert> {
    let window_size = args.window_size_ms / args.sensor_sampling_interval_ms as u64;
    debug!(
        "Window size: {window_size}, start time: {}",
        start_time.as_secs_f64()
    );
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
                trace!(
                    "Sensor {seed} read {sensor_reading} at {}",
                    time.as_secs_f64()
                );
                buffer[j as usize].push((time, sensor_reading));
                time += Duration::from_millis(args.sensor_sampling_interval_ms as u64);
            }
        }
        alerts.append(&mut get_motor_alerts(i, buffer, window_size));
    }
    alerts.sort_by_key(|alert| alert.time.round() as u64);
    alerts
}

fn get_motor_alerts(
    motor_id: u16,
    buffer: [Vec<(Duration, f32)>; 4],
    window_size: u64,
) -> Vec<Alert> {
    let mut alerts = Vec::new();
    for i in 0..buffer[0].len() {
        let air_temperature = get_average_value(i, window_size, &buffer[0]);
        let process_temperature = get_average_value(i, window_size, &buffer[1]);
        let rotational_speed = get_average_value(i, window_size, &buffer[2]);
        let torque = get_average_value(i, window_size, &buffer[3]);
        let time = buffer[0][i].0;
        if let Some(motor_failure) = utils::averages_indicate_failure(
            air_temperature,
            process_temperature,
            rotational_speed,
            torque,
            window_size as usize,
        ) {
            alerts.push(Alert {
                time: time.as_secs_f64(),
                motor_id,
                failure: motor_failure,
            })
        }
    }
    alerts
}

fn get_alert_failures<'a>(
    received_alerts: &'a [Alert],
    expected_alerts: &'a [Alert],
    duration: Duration,
) -> Vec<(String, &'a Alert)> {
    let mut received_alerts: Vec<&Alert> = received_alerts.iter().collect();
    received_alerts.sort_by(|alert_a, alert_b| alert_a.time.total_cmp(&alert_b.time));

    let mut expected_alerts: Vec<&Alert> = expected_alerts.iter().collect();
    expected_alerts.sort_by(|alert_a, alert_b| alert_a.time.total_cmp(&alert_b.time));

    let mut alert_failures = Vec::new();
    let mut received_alert_pointer = 0;
    let mut expected_alert_pointer = 0;

    loop {
        let received_alert = received_alerts.get(received_alert_pointer);
        let expected_alert = expected_alerts.get(expected_alert_pointer);
        if received_alert.is_none() || expected_alert.is_none() {
            break;
        }
        let received_alert = *received_alert.unwrap();
        let expected_alert = *expected_alert.unwrap();
        if alert_equals(received_alert, expected_alert, duration) {
            received_alert_pointer += 1;
            expected_alert_pointer += 1;
        } else if received_alert.time < expected_alert.time {
            alert_failures.push(("Received".to_string(), received_alert));
            received_alert_pointer += 1;
        } else {
            alert_failures.push(("Expected".to_string(), expected_alert));
            expected_alert_pointer += 1;
        }
    }
    alert_failures.append(
        &mut received_alerts[received_alert_pointer..]
            .iter()
            .map(|alert| ("Received".to_string(), *alert))
            .collect(),
    );
    alert_failures.append(
        &mut expected_alerts[expected_alert_pointer..]
            .iter()
            .map(|alert| ("Expected".to_string(), *alert))
            .collect(),
    );
    alert_failures
}

fn alert_equals(
    received_alert: &Alert,
    expected_alert: &Alert,
    validation_window: Duration,
) -> bool {
    expected_alert.failure == received_alert.failure
        && expected_alert.motor_id == received_alert.motor_id
        && (expected_alert.time - received_alert.time).abs() <= validation_window.as_secs_f64()
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
