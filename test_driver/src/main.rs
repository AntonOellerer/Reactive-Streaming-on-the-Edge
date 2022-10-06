use std::io::Write;
use std::net::TcpStream;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, thread};

use clap::Parser;
use libc::time_t;
use log::{error, info};
use postcard::to_allocvec_cobs;
use serde::Deserialize;

use data_transfer_objects::{
    CloudServerRunParameters, MotorDriverRunParameters, RequestProcessingModel,
};

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// Number of motor groups
    #[clap(short, long, value_parser, default_value_t = 1)]
    motor_groups: u16,

    /// Sensor sampling interval in milliseconds
    #[clap(short, long, value_parser, default_value_t = 1000)]
    sampling_interval: u32,

    /// Request Processing Model to use
    #[clap(value_enum, value_parser = parse_request_processing_model, possible_values = ["ClientServer", "ReactiveStreaming"])]
    request_processing_model: RequestProcessingModel,
}

#[derive(Deserialize)]
struct Config {
    test_run: TestRunConfig,
    motor_monitor: MotorMonitorConfig,
    motor_driver: MotorDriverConfig,
    cloud_server: CloudServerConfig,
}

#[derive(Deserialize)]
struct TestRunConfig {
    start_delay: u16,
    duration: u64,
}

#[derive(Deserialize)]
struct MotorMonitorConfig {
    window_size: i64,
    sensors_start_port: u16,
}

#[derive(Deserialize)]
struct MotorDriverConfig {
    test_driver_port: u16,
    sensor_driver_port: u16,
}

#[derive(Deserialize)]
struct CloudServerConfig {
    motor_monitor_port: u16,
    test_driver_port: u16,
}

fn parse_request_processing_model(s: &str) -> Result<RequestProcessingModel, String> {
    let result = RequestProcessingModel::from_str(s);
    match result {
        Ok(result) => Ok(result),
        Err(_) => Err(String::from(
            "Request Processing Model has to be either 'ReactiveStreaming' or 'ClientServer'",
        )),
    }
}

fn main() {
    env_logger::init();
    let args = Args::parse();
    let config: Config = toml::from_str(
        &fs::read_to_string("resources/config.toml").expect("Could not read config file"),
    )
    .expect("Could not parse config file");
    let start_time = get_now() + config.test_run.start_delay as i64;
    let motor_driver_parameters = create_motor_driver_parameters(&args, &config, start_time);
    let cloud_server_parameters: CloudServerRunParameters =
        create_cloud_server_parameters(&args, &config, start_time);
    send_motor_driver_parameters(motor_driver_parameters, &config);
    send_cloud_server_parameters(cloud_server_parameters, &config);
    thread::sleep(Duration::from_secs(config.test_run.duration));
    info!("Done");
    //todo read monitor and cloud server results
}

fn create_motor_driver_parameters(
    args: &Args,
    config: &Config,
    start_time: time_t,
) -> MotorDriverRunParameters {
    MotorDriverRunParameters {
        start_time,
        duration: config.test_run.duration,
        number_of_motor_groups: args.motor_groups as usize,
        window_size: config.motor_monitor.window_size,
        sensor_start_port: config.motor_monitor.sensors_start_port,
        sampling_interval: args.sampling_interval,
        request_processing_model: RequestProcessingModel::ReactiveStreaming,
        cloud_server_port: config.cloud_server.motor_monitor_port,
        sensor_driver_start_port: config.motor_driver.sensor_driver_port,
    }
}

fn send_motor_driver_parameters(
    motor_driver_parameters: MotorDriverRunParameters,
    config: &Config,
) {
    match TcpStream::connect(format!(
        "localhost:{}",
        config.motor_driver.test_driver_port
    )) {
        Ok(mut stream) => {
            let data = to_allocvec_cobs(&motor_driver_parameters)
                .expect("Could not write motor diver parameters to bytes");
            stream
                .write_all(&data)
                .expect("Could not send parameters to sensor driver");
            info!("Sent motor server parameters")
        }
        Err(e) => {
            error!("Failed to connect: {}", e);
        }
    }
}

fn create_cloud_server_parameters(
    args: &Args,
    config: &Config,
    start_time: time_t,
) -> CloudServerRunParameters {
    CloudServerRunParameters {
        start_time,
        duration: config.test_run.duration,
        motor_monitor_port: config.cloud_server.motor_monitor_port,
        request_processing_model: args.request_processing_model,
    }
}

fn send_cloud_server_parameters(
    cloud_server_parameters: CloudServerRunParameters,
    config: &Config,
) {
    match TcpStream::connect(format!(
        "localhost:{}",
        config.cloud_server.test_driver_port
    )) {
        Ok(mut stream) => {
            let data = to_allocvec_cobs(&cloud_server_parameters)
                .expect("Could not write motor diver parameters to bytes");
            stream
                .write_all(&data)
                .expect("Could not send parameters to sensor driver");
            info!("Sent cloud server parameters")
        }
        Err(e) => {
            error!("Failed to connect: {}", e);
        }
    }
}

pub fn get_now() -> time_t {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Could not get epoch seconds")
        .as_secs()
        .try_into()
        .expect("Could not convert now start to time_t")
}
