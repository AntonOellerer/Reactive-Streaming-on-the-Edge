use std::{fs, thread};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::net::TcpStream;
use std::str::FromStr;

use clap::Parser;
use libc::time_t;
use log::info;
use postcard::to_allocvec_cobs;
use serde::Deserialize;

use data_transfer_objects::{
    BenchmarkData, BenchmarkDataType, CloudServerRunParameters, MotorDriverRunParameters,
    RequestProcessingModel,
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
    let start_time = utils::get_now() + config.test_run.start_delay as i64;
    let mut motor_driver_connection = connect_to_driver(config.motor_driver.test_driver_port);
    let mut cloud_server_connection = connect_to_driver(config.cloud_server.test_driver_port);
    let motor_driver_parameters = create_motor_driver_parameters(&args, &config, start_time);
    let cloud_server_parameters: CloudServerRunParameters =
        create_cloud_server_parameters(&args, &config, start_time);
    send_motor_driver_parameters(motor_driver_parameters, &mut motor_driver_connection);
    send_cloud_server_parameters(cloud_server_parameters, &mut cloud_server_connection);
    thread::sleep(utils::get_sleep_duration(
        start_time,
        config.test_run.duration,
    ));
    info!("Done");
    save_benchmark_results(args.motor_groups, &mut motor_driver_connection);
    // get_alert_results();
}

fn connect_to_driver(port: u16) -> TcpStream {
    TcpStream::connect(format!("localhost:{}", port))
        .unwrap_or_else(|_| panic!("Could not connect to {}", port))
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
    tcp_stream: &mut TcpStream,
) {
    let data = to_allocvec_cobs(&motor_driver_parameters)
        .expect("Could not write motor diver parameters to bytes");
    tcp_stream
        .write_all(&data)
        .expect("Could not send parameters to sensor driver");
    info!("Sent motor server parameters")
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
    tcp_stream: &mut TcpStream,
) {
    let data = to_allocvec_cobs(&cloud_server_parameters)
        .expect("Could not write motor diver parameters to bytes");
    tcp_stream
        .write_all(&data)
        .expect("Could not send parameters to sensor driver");
    info!("Sent cloud server parameters")
}

fn save_benchmark_results(motor_groups: u16, tcp_stream: &mut TcpStream) {
    let mut motor_monitor_benchmark_data = open_results_file("motor_monitor_results.csv");
    let mut sensor_benchmark_data = open_results_file("sensor_benchmark_data_results.csv");
    for _ in 0..(motor_groups * 4 + 1) {
        let benchmark_data = utils::read_object::<BenchmarkData>(tcp_stream)
            .expect("Could not read sensor benchmark data");
        if benchmark_data.benchmark_data_type == BenchmarkDataType::Sensor {
            sensor_benchmark_data
                .write_all(benchmark_data.to_csv_string().as_bytes())
                .expect("Could not write sensor benchmark data");
        } else {
            motor_monitor_benchmark_data
                .write_all(benchmark_data.to_csv_string().as_bytes())
                .expect("Could not write motor monitor benchmark data");
        }
    }
    info!("Read benchmark data");
}

fn open_results_file(file_name: &str) -> File {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(file_name)
        .expect("Could not open results protocol file for writing")
}
