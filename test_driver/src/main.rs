use clap::builder::TypedValueParser;
use clap::Parser;
use data_transfer_objects::{
    Alert, BenchmarkData, BenchmarkDataType, CloudServerRunParameters, MotorDriverRunParameters,
    MotorSensorGroup, RequestProcessingModel,
};
use log::{debug, info};
use postcard::to_allocvec_cobs;
use serde::Deserialize;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::str;
use std::str::FromStr;
use std::time::Duration;
use std::{fs, thread};

mod validator;

#[cfg(debug_assertions)]
const CONFIG_PATH: &str = "resources/config-debug.toml";
#[cfg(not(debug_assertions))]
const CONFIG_PATH: &str = "resources/config-production.toml";

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// Number of motor groups connected via tcp
    #[clap(long, value_parser, default_value_t = 1, short)]
    motor_groups_tcp: u16,

    /// Number of motor groups connected via i2c
    #[clap(long, value_parser, default_value_t = 0)]
    motor_groups_i2c: u8,

    /// Sensor sampling interval in milliseconds
    #[clap(short, long, value_parser, default_value_t = 30)]
    duration: u64,

    /// Request Processing Model to use
    #[clap(value_enum, value_parser = clap::builder::PossibleValuesParser::new(["ClientServer", "ReactiveStreaming"]).map(| s | parse_request_processing_model(& s)))]
    request_processing_model: RequestProcessingModel,

    // Size of the window averaged for determining sensor reading value
    #[clap(short, long, value_parser, default_value_t = 3)]
    window_size_seconds: u64,

    // Sampling interval of window and sensor
    #[clap(short, long, value_parser, default_value_t = 1000)]
    sampling_interval: u32,

    // Size of the thread pool
    #[clap(short, long, value_parser, default_value_t = 40)]
    thread_pool_size: usize,
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
    start_delay: u64,
}

#[derive(Deserialize)]
struct MotorMonitorConfig {
    sensor_listen_address: SocketAddr,
}

#[derive(Deserialize)]
struct MotorDriverConfig {
    test_driver_listen_address: SocketAddr,
    motor_sensor_groups: Vec<MotorSensorGroup>,
}

#[derive(Deserialize)]
struct CloudServerConfig {
    motor_monitor_listen_address: SocketAddr,
    test_driver_listen_address: SocketAddr,
}

fn parse_request_processing_model(s: &str) -> RequestProcessingModel {
    RequestProcessingModel::from_str(s).expect("Could not parse RequestProcessingModel")
}

fn main() {
    env_logger::init();
    let args = Args::parse();
    let config: Config =
        toml::from_str(&fs::read_to_string(CONFIG_PATH).expect("Could not read config file"))
            .expect("Could not parse config file");
    execute_benchmark_run(&args, &config);
}

fn execute_benchmark_run(args: &Args, config: &Config) {
    let start_time = utils::get_now_duration() + Duration::from_secs(config.test_run.start_delay);

    let mut motor_driver_connection = setup_motor_driver(args, config, start_time);
    let mut cloud_server_connection = setup_cloud_server(args, config, start_time);

    thread::sleep(utils::get_duration_to_end(
        start_time,
        Duration::from_secs(args.duration),
    ));

    info!("Saving benchmark results");
    save_benchmark_results(args.motor_groups_tcp, &mut motor_driver_connection);
    info!("Getting alerts");
    let alerts = get_alerts(&mut cloud_server_connection);
    info!("Validating alerts");
    validator::validate_alerts(args, start_time, &alerts);
}

fn setup_motor_driver(args: &Args, config: &Config, start_time: Duration) -> TcpStream {
    let mut motor_driver_connection =
        connect_to_remote(config.motor_driver.test_driver_listen_address);
    let motor_driver_parameters =
        create_motor_driver_parameters(args, config, start_time.as_secs_f64());
    send_motor_driver_parameters(motor_driver_parameters, &mut motor_driver_connection);
    motor_driver_connection
}

fn setup_cloud_server(args: &Args, config: &Config, start_time: Duration) -> TcpStream {
    let mut cloud_server_connection =
        connect_to_remote(config.cloud_server.test_driver_listen_address);
    let cloud_server_parameters: CloudServerRunParameters =
        create_cloud_server_parameters(args, config, start_time.as_secs_f64());
    send_cloud_server_parameters(cloud_server_parameters, &mut cloud_server_connection);
    cloud_server_connection
}

fn connect_to_remote(address: SocketAddr) -> TcpStream {
    TcpStream::connect(format!("127.0.0.1:{}", address.port()))
        .unwrap_or_else(|_| panic!("Could not connect to {address}"))
}

fn create_motor_driver_parameters(
    args: &Args,
    config: &Config,
    start_time: f64,
) -> MotorDriverRunParameters {
    MotorDriverRunParameters {
        start_time,
        duration: Duration::from_secs(args.duration).as_secs_f64(),
        number_of_tcp_motor_groups: args.motor_groups_tcp as usize,
        number_of_i2c_motor_groups: args.motor_groups_i2c,
        window_size_seconds: Duration::from_secs(args.window_size_seconds).as_secs_f64(),
        sensor_listen_address: config.motor_monitor.sensor_listen_address,
        sampling_interval: args.sampling_interval,
        request_processing_model: args.request_processing_model,
        motor_monitor_listen_address: config.cloud_server.motor_monitor_listen_address,
        motor_sensor_groups: config.motor_driver.motor_sensor_groups.clone(),
        thread_pool_size: args.thread_pool_size,
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
    start_time: f64,
) -> CloudServerRunParameters {
    CloudServerRunParameters {
        start_time,
        duration: Duration::from_secs(args.duration).as_secs_f64(),
        motor_monitor_listen_address: config.cloud_server.motor_monitor_listen_address,
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

fn get_alerts(cloud_server_stream: &mut TcpStream) -> Vec<Alert> {
    let mut buffer = Vec::new();
    let _ = cloud_server_stream
        .read_to_end(&mut buffer)
        .expect("Could not get alert file from cloud server");
    let alerts = str::from_utf8(&buffer).expect("Could not convert u8 buffer to string");
    debug!("{:?}", alerts);
    alerts
        .lines()
        .map(|line| Alert::from_csv(String::from(line)))
        .collect()
}
