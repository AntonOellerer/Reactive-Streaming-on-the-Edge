use chrono::NaiveDateTime;
use env_logger::Target;
use log::{debug, info};
use postcard::to_allocvec_cobs;
use rand::prelude::IteratorRandom;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use std::io::{BufRead, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use std::{fs, thread};

use data_transfer_objects::{RequestProcessingModel, SensorMessage, SensorParameters};

fn main() {
    env_logger::builder().target(Target::Stderr).init();
    let arguments: Vec<String> = std::env::args().collect();
    let data_path = get_and_validate_path(&arguments);

    let sensor_parameters: SensorParameters = get_sensor_parameters(&arguments);
    let mut rng = SmallRng::seed_from_u64(sensor_parameters.id as u64);

    execute_client_server_procedure(data_path, &sensor_parameters, &mut rng);
    info!("Finished benchmark run");
}

fn get_and_validate_path(args: &[String]) -> &Path {
    let path = args.get(1).expect("Did not receive at least 1 argument");
    let path = Path::new(path);
    let path_valid = path.try_exists();
    assert!(path_valid.is_ok() & path_valid.expect("Invalid data file path given to sensor"));
    path
}

fn get_sensor_parameters(arguments: &[String]) -> SensorParameters {
    SensorParameters {
        id: arguments
            .get(2)
            .expect("Did not receive at least 2 arguments")
            .parse()
            .expect("Could not parse id successfully"),
        duration: arguments
            .get(3)
            .expect("Did not receive at least 3 arguments")
            .parse()
            .expect("Could not parse duration successfully"),
        sampling_interval: arguments
            .get(4)
            .expect("Did not receive at least 4 arguments")
            .parse()
            .expect("Could not parse sampling interval successfully"),
        request_processing_model: RequestProcessingModel::from_str(
            arguments
                .get(5)
                .expect("Did not receive at least 5 arguments"),
        )
        .expect("Could not parse Request Processing Model successfully"),
        motor_monitor_listen_address: arguments
            .get(6)
            .expect("Did not receive at least 6 arguments")
            .parse()
            .expect("Could not parse motor monitor listen address successfully"),
        start_time: arguments
            .get(7)
            .expect("Did not receive at least 7 arguments")
            .parse()
            .expect("Could not parse start time successfully"),
    }
}

fn get_monitor_connection(sensor_parameters: &SensorParameters) -> TcpStream {
    let connect_to = format!(
        "{}:{}",
        get_monitor_address(sensor_parameters.motor_monitor_listen_address.ip()),
        sensor_parameters.motor_monitor_listen_address.port(),
    )
    .to_socket_addrs()
    .unwrap()
    .next()
    .unwrap();
    thread::sleep(Duration::from_secs(2));
    TcpStream::connect_timeout(&connect_to, Duration::from_secs(5))
        .unwrap_or_else(|e| panic!("Could not connect to {connect_to:?}: {e}"))
}

#[cfg(debug_assertions)]
fn get_monitor_address(addr: IpAddr) -> String {
    addr.to_string()
}

#[cfg(not(debug_assertions))]
fn get_monitor_address(_addr: IpAddr) -> String {
    "bench_system_monitor".to_string()
}

fn execute_client_server_procedure(
    data_path: &Path,
    sensor_parameters: &SensorParameters,
    mut rng: &mut SmallRng,
) {
    let start_time = Duration::from_secs_f64(sensor_parameters.start_time);
    let end_time = start_time + Duration::from_secs_f64(sensor_parameters.duration);
    debug!(
        "Sleeping for {}",
        (start_time - utils::get_now_duration()).as_secs_f64()
    );
    thread::sleep(start_time - utils::get_now_duration());
    let mut stream = get_monitor_connection(sensor_parameters);
    info!(
        "Connected to {}",
        sensor_parameters.motor_monitor_listen_address
    );
    while utils::get_now_duration() < end_time {
        let sensor_reading = fs::read(data_path)
            .expect("Failure reading sensor data")
            .lines()
            .choose_stable(&mut rng)
            .expect("Data file iterator is empty")
            .expect("Error reading from data file iterator")
            .parse()
            .expect("Error parsing data fileline");
        send_sensor_reading(sensor_parameters, sensor_reading, &mut stream);
        thread::sleep(Duration::from_millis(
            sensor_parameters.sampling_interval as u64,
        ))
    }
}

fn send_sensor_reading(
    sensor_parameters: &SensorParameters,
    sensor_reading: f32,
    stream: &mut TcpStream,
) {
    let message = SensorMessage {
        reading: sensor_reading,
        sensor_id: sensor_parameters.id,
        timestamp: utils::get_now_duration().as_secs_f64(),
    };
    debug!("Read {sensor_reading} at {}", message.timestamp);
    let vec: Vec<u8> = match sensor_parameters.request_processing_model {
        RequestProcessingModel::ReactiveStreaming => {
            to_allocvec_cobs(&message).expect("Could not write sensor reading to Vec<u8>")
        }
        RequestProcessingModel::ClientServer => {
            to_allocvec_cobs(&message).expect("Could not write sensor reading to Vec<u8>")
        }
        RequestProcessingModel::ObjectOriented => {
            to_allocvec_cobs(&message).expect("Could not write sensor reading to Vec<u8>")
        }
        RequestProcessingModel::SpringQL => jsonify(message).as_bytes().to_vec(),
    };
    stream
        .write_all(&vec)
        .expect("Could not write sensor reading bytes to TcpStream");
}

fn jsonify(message: SensorMessage) -> String {
    format!(
        "{{\"ts\": \"{}\", \"reading\": {}, \"sensor_id\": {}}}\n",
        to_rfc3339(message),
        message.reading,
        message.sensor_id
    )
}

fn to_rfc3339(message: SensorMessage) -> String {
    NaiveDateTime::from_timestamp_millis(
        Duration::from_secs_f64(message.timestamp).as_millis() as i64
    )
    .expect("Could not convert f64 to chrono::Duration")
    .and_local_timezone(chrono::offset::Utc)
    .unwrap()
    .to_rfc3339()
}
