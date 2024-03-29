use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::ops::Shl;
use std::str::FromStr;
use std::sync::mpsc;
use std::time::Duration;

use env_logger::Target;
use futures::executor::{ThreadPool, ThreadPoolBuilder};
use futures::future::RemoteHandle;
use log::{debug, info};

use data_transfer_objects::{BenchmarkDataType, MotorMonitorParameters};
use scheduler::Scheduler;

mod monitor;
mod sensor;

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
    let listen_address = SocketAddr::new(
        IpAddr::from_str("0.0.0.0").unwrap(),
        motor_monitor_parameters.sensor_listen_address.port(),
    );
    let listener = TcpListener::bind(listen_address).unwrap();
    debug!("Bound to {:?}", listen_address);
    let mut handles = vec![];
    for motor_id in 0..motor_monitor_parameters.number_of_tcp_motor_groups {
        let (sender, receiver) = mpsc::channel();
        let monitor = monitor::MotorMonitor::build(receiver, cloud_server.try_clone().unwrap());
        handles.push(thread_pool.schedule(move || monitor.run()));
        for sensor_id in 0..4 {
            let full_id: u32 = (motor_id as u32).shl(2) + sensor_id as u32;
            let sensor = sensor::Sensor::build(
                Duration::from_millis(motor_monitor_parameters.window_size_ms),
                Duration::from_millis(motor_monitor_parameters.window_sampling_interval as u64),
                sender.clone(),
                listener.try_clone().unwrap(),
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
