use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::str;
use std::str::FromStr;
use std::time::Duration;
use std::{fs, thread};

use clap::builder::TypedValueParser;
use clap::Parser;
use log::{debug, info};
use postcard::to_allocvec_cobs;
use serde::Deserialize;

use data_transfer_objects::{
    Alert, AlertWithDelay, BenchmarkData, CloudServerRunParameters, MotorDriverRunParameters,
    NetworkConfig, RequestProcessingModel,
};

#[cfg(debug_assertions)]
const CONFIG_PATH: &str = "resources/config-debug.toml";
#[cfg(not(debug_assertions))]
const NETWORK_CONFIG_PATH: &str = "../network_config.toml";
#[cfg(debug_assertions)]
const MONITOR_IP: &str = "127.0.0.1";
#[cfg(not(debug_assertions))]
const MONITOR_IP: &str = "192.168.178.51";

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
    #[clap(value_enum, value_parser = clap::builder::PossibleValuesParser::new(["ClientServer", "ReactiveStreaming", "SpringQL", "ObjectOriented"]).map(| s | parse_request_processing_model(& s)))]
    request_processing_model: RequestProcessingModel,

    /// Size of the window averaged for determining sensor reading value
    #[clap(long, value_parser, default_value_t = 3000)]
    window_size_ms: u64,

    /// Window sampling interval in milliseconds
    #[clap(short, long, value_parser, default_value_t = 1000)]
    window_sampling_interval_ms: u32,

    /// Sampling interval of sensor in milliseconds
    #[clap(short, long, value_parser, default_value_t = 1000)]
    sensor_sampling_interval_ms: u32,

    /// Size of the thread pool
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
    sensor_socket_addresses: Vec<SocketAddr>,
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
    let config: Config = get_config();
    execute_benchmark_run(&args, &config);
}

#[cfg(debug_assertions)]
fn get_config() -> Config {
    toml::from_str(&fs::read_to_string(CONFIG_PATH).expect("Could not read config file"))
        .expect("Could not parse config file")
}

#[cfg(not(debug_assertions))]
fn get_config() -> Config {
    let network: NetworkConfig = toml::from_str(
        &fs::read_to_string(NETWORK_CONFIG_PATH).expect("Could not read config file"),
    )
    .expect("Could not parse config file");
    Config {
        test_run: TestRunConfig { start_delay: 5 },
        motor_monitor: MotorMonitorConfig {
            sensor_listen_address: SocketAddr::new(network.motor_monitor_address, 9000),
        },
        motor_driver: MotorDriverConfig {
            test_driver_listen_address: SocketAddr::new(network.motor_monitor_address, 8000),
            sensor_socket_addresses: network
                .sensor_addresses
                .iter()
                .map(|ip_addr| SocketAddr::new(*ip_addr, 11000))
                .collect(),
        },
        cloud_server: CloudServerConfig {
            motor_monitor_listen_address: SocketAddr::new(network.cloud_server_address, 10000),
            test_driver_listen_address: SocketAddr::new(network.cloud_server_address, 8001),
        },
    }
}

fn execute_benchmark_run(args: &Args, config: &Config) {
    let start_delay = match args.request_processing_model {
        RequestProcessingModel::ReactiveStreaming => config.test_run.start_delay,
        RequestProcessingModel::ClientServer => config.test_run.start_delay,
        RequestProcessingModel::SpringQL => (args.motor_groups_tcp * 4 * 4) as u64, //each sensor port takes about 4 seconds to open
        RequestProcessingModel::ObjectOriented => config.test_run.start_delay,
    };
    let start_time = utils::get_now_duration() + Duration::from_secs(start_delay);

    let mut motor_driver_connection = setup_motor_driver(args, config, start_time);
    let mut cloud_server_connection = setup_cloud_server(args, config, start_time);

    thread::sleep(utils::get_duration_to_end(
        start_time,
        Duration::from_secs(args.duration),
    ));

    save_benchmark_results(&mut motor_driver_connection);
    info!("Saved benchmark results");
    let (_alerts, delays) = get_alerts_with_delays(&mut cloud_server_connection);
    info!("Fetched alerts");
    // let failures = validator::validate_alerts(args, start_time, &alerts);
    info!("Validated alerts");
    persist_delays(delays);
    // persist_failures(failures);
    info!("Finished test run");
}

fn setup_motor_driver(args: &Args, config: &Config, start_time: Duration) -> TcpStream {
    let mut motor_driver_connection = connect_to_remote(
        SocketAddr::from_str(
            format!(
                "{MONITOR_IP}:{}",
                config.motor_driver.test_driver_listen_address.port()
            )
            .as_str(),
        )
        .unwrap(),
    ); //todo
    let motor_driver_parameters =
        create_motor_driver_parameters(args, config, start_time.as_secs_f64());
    send_motor_driver_parameters(motor_driver_parameters, &mut motor_driver_connection);
    motor_driver_connection
}

fn setup_cloud_server(args: &Args, config: &Config, start_time: Duration) -> TcpStream {
    let mut cloud_server_connection = connect_to_remote(
        SocketAddr::from_str(
            format!(
                "127.0.0.1:{}",
                config.cloud_server.test_driver_listen_address.port()
            )
            .as_str(),
        )
        .unwrap(),
    );
    let cloud_server_parameters: CloudServerRunParameters =
        create_cloud_server_parameters(args, config, start_time.as_secs_f64());
    send_cloud_server_parameters(cloud_server_parameters, &mut cloud_server_connection);
    cloud_server_connection
}

fn connect_to_remote(address: SocketAddr) -> TcpStream {
    info!("Connecting to {address}");
    let stream =
        TcpStream::connect(address).unwrap_or_else(|_| panic!("Could not connect to {address}"));
    info!("Connected to {address}");
    stream
}

fn create_motor_driver_parameters(
    args: &Args,
    config: &Config,
    start_time: f64,
) -> MotorDriverRunParameters {
    let sensor_socket_addresses = match !config.motor_driver.sensor_socket_addresses.is_empty() {
        true => config.motor_driver.sensor_socket_addresses.clone(),
        false => fs::read_to_string("sensor_socket_addresses.txt")
            .unwrap()
            .lines()
            .map(|line| SocketAddr::from_str(line).unwrap())
            .collect(),
    };
    MotorDriverRunParameters {
        start_time,
        duration: Duration::from_secs(args.duration).as_secs_f64(),
        number_of_tcp_motor_groups: args.motor_groups_tcp as usize,
        number_of_i2c_motor_groups: args.motor_groups_i2c,
        window_size_ms: args.window_size_ms,
        sensor_listen_address: config.motor_monitor.sensor_listen_address,
        sensor_sampling_interval: args.sensor_sampling_interval_ms,
        window_sampling_interval: args.window_sampling_interval_ms,
        request_processing_model: args.request_processing_model,
        motor_monitor_listen_address: config.cloud_server.motor_monitor_listen_address,
        sensor_socket_addresses,
        thread_pool_size: args.thread_pool_size,
    }
}

fn send_motor_driver_parameters(
    motor_driver_parameters: MotorDriverRunParameters,
    tcp_stream: &mut TcpStream,
) {
    let data = to_allocvec_cobs(&motor_driver_parameters)
        .expect("Could not write motor diver parameters to bytes");
    debug!("Motor driver parameters size: {}", data.len());
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

fn save_benchmark_results(tcp_stream: &mut TcpStream) {
    let mut motor_monitor_benchmark_data = open_results_file("motor_monitor_results.csv");
    let benchmark_data =
        utils::read_object::<BenchmarkData>(tcp_stream).expect("Could not read benchmark data");
    motor_monitor_benchmark_data
        .write_all(benchmark_data.to_csv_string().as_bytes())
        .expect("Could not write motor monitor benchmark data");
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

fn get_alerts_with_delays(cloud_server_stream: &mut TcpStream) -> (Vec<Alert>, Vec<f64>) {
    let mut buffer = Vec::new();
    let _ = cloud_server_stream
        .read_to_end(&mut buffer)
        .expect("Could not get alert file from cloud server");
    let alerts = str::from_utf8(&buffer).expect("Could not convert u8 buffer to string");
    debug!("{:?}", alerts);
    let alerts_with_delays: Vec<AlertWithDelay> = alerts
        .lines()
        .map(|line| AlertWithDelay::from_csv(String::from(line)))
        .collect();
    let mut alerts = vec![];
    let mut delays = vec![];
    for alert_with_delay in alerts_with_delays {
        delays.push(alert_with_delay.delay);
        alerts.push(Alert::from_alert_with_delay(alert_with_delay));
    }
    (alerts, delays)
}

fn persist_delays(delays: Vec<f64>) {
    if !delays.is_empty() {
        let mut delay_file = open_results_file("alert_delays.csv");
        write!(
            delay_file,
            "{},",
            delays
                .iter()
                .map(|delay| delay.to_string())
                .collect::<Vec<String>>()
                .join(",")
        )
        .expect("Could not write to alert delays file");
    }
}

// While it does not really make sense to persist a single value to a file,
// this is done so that the external interface stays the same over the different
// result metrics of the service (resource usage, delays, failures)
// fn persist_failures(failures: usize) {
//     let mut failure_file = open_results_file("alert_failures.csv");
//     write!(failure_file, "{failures},").expect("Could not write to failures file");
// }
