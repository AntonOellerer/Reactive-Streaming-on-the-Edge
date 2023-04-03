#![feature(let_chains)]

mod monitor;
mod sensor;

use data_transfer_objects::{Alert, BenchmarkDataType, MotorFailure, MotorMonitorParameters};
use env_logger::Target;
use futures::executor::{ThreadPool, ThreadPoolBuilder};
use futures::future::RemoteHandle;
use log::{debug, info};
use scheduler::Scheduler;
use std::net::{SocketAddr, TcpStream};
use std::ops::{BitAnd, Shl, Shr};
use std::sync::mpsc;
use std::time::Duration;

fn main() {
    env_logger::builder().target(Target::Stderr).init();
    let arguments: Vec<String> = std::env::args().collect();
    let motor_monitor_parameters: MotorMonitorParameters =
        utils::get_motor_monitor_parameters(&arguments);
    info!("Running procedure");
    execute_procedure(motor_monitor_parameters);
    info!("Processing completed");
    utils::save_benchmark_readings(0, BenchmarkDataType::MotorMonitor);
    info!("Saved benchmark readings");
}

fn execute_procedure(motor_monitor_parameters: MotorMonitorParameters) {
    let pool = ThreadPoolBuilder::new()
        .pool_size(motor_monitor_parameters.thread_pool_size)
        .create()
        .unwrap();
    let handle_list = setup_threads(motor_monitor_parameters, pool);
    wait_on_complete(handle_list);
}

fn setup_threads(
    motor_monitor_parameters: MotorMonitorParameters,
    thread_pool: ThreadPool,
) -> Vec<RemoteHandle<()>> {
    let cloud_server = TcpStream::connect(motor_monitor_parameters.motor_monitor_listen_address)
        .expect("Could not open connection to cloud server");
    info!(
        "Connected to {}",
        motor_monitor_parameters.motor_monitor_listen_address
    );
    let mut handles = vec![];
    for motor_id in 0..motor_monitor_parameters.number_of_tcp_motor_groups {
        let (sender, receiver) = mpsc::channel();
        let monitor = monitor::MotorMonitor {
            // motor_id: 0,
            sensor_data_receiver: receiver,
            cloud_server: cloud_server.try_clone().unwrap(),
            air_temperature: None,
            process_temperature: None,
            rotational_speed: None,
            torque: None,
            age: utils::get_now_duration(),
        };
        handles.push(thread_pool.schedule(move || monitor.run()));
        for sensor_id in 0..4 {
            let full_id: u32 = (motor_id as u32).shl(2) + sensor_id as u32;
            debug!("Full id: {full_id}");
            let sensor = sensor::Sensor::build(
                Duration::from_millis(motor_monitor_parameters.window_size_ms),
                sender.clone(),
                SocketAddr::new(
                    motor_monitor_parameters.sensor_listen_address.ip(),
                    motor_monitor_parameters.sensor_listen_address.port() + full_id as u16,
                ),
            );
            handles.push(thread_pool.schedule(move || sensor.run()))
        }
    }
    handles
}

fn wait_on_complete(handle_list: Vec<RemoteHandle<()>>) {
    for handle in handle_list {
        futures::executor::block_on(handle);
    }
}
