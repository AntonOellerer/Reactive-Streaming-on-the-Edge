use std::fs::File;
use std::io::{Read, Write};
use std::mem::size_of;
use std::net::TcpStream;
use std::ops::Shl;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use data_transfer_objects::RequestProcessingModel::ClientServer;
use data_transfer_objects::{
    MotorDriverParameters, MotorMonitorParameters, SensorBenchmarkData, SensorParameters,
};
use postcard::{from_bytes, to_allocvec};
use threadpool::ThreadPool;

fn main() {
    let motor_driver_parameters: MotorDriverParameters = serde_json::from_reader(
        File::open("resources/config.json").expect("Could not open config file"),
    )
    .expect("Could not parse MotorDriverParameters from json config file");
    let motor_monitor_parameters = create_motor_monitor_parameters(&motor_driver_parameters);
    let no_of_sensors = motor_driver_parameters.number_of_motor_groups * 4;
    let pool = ThreadPool::new(no_of_sensors as usize);
    for motor_id in 0..motor_driver_parameters.number_of_motor_groups as u16 {
        for sensor_id in 0..4u16 {
            let full_id: u32 = (motor_id as u32).shl(16) + sensor_id as u32;
            let driver_port: u16 = motor_driver_parameters.driver_port + motor_id * 5 + sensor_id;
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
    motor_driver_parameters: &MotorDriverParameters,
) {
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
            eprintln!("Failed to connect: {}", e);
        }
    }
}

fn create_motor_monitor_parameters(
    motor_driver_parameters: &MotorDriverParameters,
) -> MotorMonitorParameters {
    MotorMonitorParameters {
        start_time: motor_driver_parameters.start_time,
        duration: motor_driver_parameters.duration,
        request_processing_model: motor_driver_parameters.request_processing_model,
        number_of_motor_groups: motor_driver_parameters.number_of_motor_groups,
        window_size: motor_driver_parameters.window_size,
        start_port: motor_driver_parameters.start_port,
        cloud_server_port: motor_driver_parameters.cloud_server_port,
    }
}

fn create_sensor_parameters(
    id: u32,
    port: u16,
    motor_driver_parameters: &MotorDriverParameters,
) -> SensorParameters {
    SensorParameters {
        id,
        start_time: motor_driver_parameters.start_time,
        duration: motor_driver_parameters.duration,
        seed: id,
        sampling_interval: motor_driver_parameters.sampling_interval,
        request_processing_model: ClientServer,
        port,
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
    println!("{:?}", benchmark_data);
}
