use std::io::{Read, Write};
use std::mem::size_of;
use std::net::{TcpListener, TcpStream};
use std::ops::Shl;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, thread};

use log::{error, info};
use postcard::{from_bytes, to_allocvec};
use serde::Deserialize;
use threadpool::ThreadPool;

use data_transfer_objects::RequestProcessingModel::ClientServer;
use data_transfer_objects::{
    MotorDriverRunParameters, MotorMonitorParameters, SensorBenchmarkData, SensorParameters,
};
use utils::get_object;

#[derive(Deserialize)]
struct MotorDriverParameters {
    test_driver_port: u16,
}

fn main() {
    env_logger::init();
    let motor_driver_parameters: MotorDriverParameters = toml::from_str(
        &fs::read_to_string("resources/config.toml").expect("Could not read config file"),
    )
    .expect("Could not parse MotorDriverParameters from json config file");
    info!(
        "Attempting to bind to port {}",
        motor_driver_parameters.test_driver_port
    );
    let listener = TcpListener::bind(format!(
        "localhost:{}",
        motor_driver_parameters.test_driver_port
    ))
    .expect("Failure binding to port");
    for control_stream in listener.incoming() {
        match control_stream {
            Ok(mut control_stream) => {
                thread::spawn(move || {
                    info!("New run");
                    let run_parameters =
                        get_object::<MotorDriverRunParameters>(&mut control_stream)
                            .expect("Could not get run parameters");
                    execute_new_run(run_parameters);
                });
            }
            Err(e) => {
                error!("Error: {}", e);
                /* connection failed */
            }
        }
    }
}

fn execute_new_run(motor_driver_parameters: MotorDriverRunParameters) {
    let motor_monitor_parameters = create_motor_monitor_parameters(&motor_driver_parameters);
    let no_of_sensors = motor_driver_parameters.number_of_motor_groups * 4;
    let pool = ThreadPool::new(no_of_sensors as usize);
    for motor_id in 0..motor_driver_parameters.number_of_motor_groups as u16 {
        for sensor_id in 0..4u16 {
            let full_id: u32 = (motor_id as u32).shl(16) + sensor_id as u32;
            let driver_port: u16 =
                motor_driver_parameters.sensor_driver_start_port + motor_id * 5 + sensor_id;
            let sensor_port: u16 = motor_monitor_parameters.start_port + motor_id * 5 + sensor_id;
            pool.execute(move || {
                control_sensor(full_id, driver_port, sensor_port, &motor_driver_parameters)
            });
        }
    }
    start_motor_monitor(motor_monitor_parameters);
    pool.join();
}

fn start_motor_monitor(motor_monitor_parameters: MotorMonitorParameters) {
    println!("Running motor monitor");
    let _output = Command::new("cargo")
        .current_dir("../motor_monitor")
        .arg("run")
        .arg("--")
        .arg(motor_monitor_parameters.start_time.to_string())
        .arg(motor_monitor_parameters.duration.to_string())
        .arg(
            motor_monitor_parameters
                .request_processing_model
                .to_string(),
        )
        .arg(motor_monitor_parameters.number_of_motor_groups.to_string())
        .arg(motor_monitor_parameters.window_size.to_string())
        .arg(motor_monitor_parameters.start_port.to_string())
        .arg(motor_monitor_parameters.cloud_server_port.to_string())
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .output()
        .expect("Failure when trying to run motor monitor program");
}

fn control_sensor(
    id: u32,
    driver_port: u16,
    sensor_port: u16,
    motor_driver_parameters: &MotorDriverRunParameters,
) {
    info!(
        "Sending info to sensor {}, driver port {}, sensor port {}",
        id, driver_port, sensor_port
    );
    let sensor_parameters = create_sensor_parameters(id, sensor_port, motor_driver_parameters);
    match TcpStream::connect(format!("localhost:{}", driver_port)) {
        Ok(mut stream) => {
            let real_start_time = Instant::now()
                + Duration::from_secs(
                    sensor_parameters
                        .start_time
                        .try_into()
                        .expect("Could not convert time_t start time to u64"),
                );
            write_sensor_parameters(&sensor_parameters, &mut stream);
            thread::sleep(
                real_start_time + Duration::from_secs(sensor_parameters.duration) - Instant::now(),
            );
            read_sensor_benchmark_data(&mut stream);
        }
        Err(e) => {
            error!("Failed to connect: {}", e);
        }
    }
}

fn create_motor_monitor_parameters(
    motor_driver_parameters: &MotorDriverRunParameters,
) -> MotorMonitorParameters {
    MotorMonitorParameters {
        start_time: motor_driver_parameters.start_time,
        duration: motor_driver_parameters.duration,
        request_processing_model: motor_driver_parameters.request_processing_model,
        number_of_motor_groups: motor_driver_parameters.number_of_motor_groups,
        window_size: motor_driver_parameters.window_size,
        start_port: motor_driver_parameters.sensor_start_port,
        cloud_server_port: motor_driver_parameters.cloud_server_port,
    }
}

fn create_sensor_parameters(
    id: u32,
    port: u16,
    motor_driver_parameters: &MotorDriverRunParameters,
) -> SensorParameters {
    SensorParameters {
        id,
        start_time: motor_driver_parameters.start_time,
        duration: motor_driver_parameters.duration,
        seed: id,
        sampling_interval: motor_driver_parameters.sampling_interval,
        request_processing_model: ClientServer,
        motor_monitor_port: port,
    }
}

fn write_sensor_parameters(sensor_parameters: &SensorParameters, stream: &mut TcpStream) {
    let vec: Vec<u8> =
        to_allocvec(&sensor_parameters).expect("Could not write sensor parameters to Vec<u8>");
    stream
        .write_all(&vec)
        .expect("Could not write sensor parameters bytes to TcpStream");
}

fn read_sensor_benchmark_data(stream: &mut TcpStream) {
    let mut data = [0; size_of::<SensorBenchmarkData>()];
    let _read = stream
        .read(&mut data)
        .expect("Could not read benchmark data bytes from TcpStream");
    let benchmark_data: SensorBenchmarkData =
        from_bytes(&data).expect("Could not parse sensor benchmark data from bytes");
    info!("{:?}", benchmark_data);
}
