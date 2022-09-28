use log::{debug, error, info};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::mem::size_of;
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use postcard::accumulator::{CobsAccumulator, FeedResult};

use data_transfer_objects::{Alert, CloudServerRunParameters};

const ALERT_SIZE: usize = size_of::<Alert>();

struct CloudServerParameters {
    driver_port: u16,
    monitor_port: u16,
}

fn main() {
    env_logger::init();
    let arguments: Vec<String> = std::env::args().collect();
    let cloud_server_parameters: CloudServerParameters = get_cloud_server_parameters(arguments);
    //todo get arguments from client
    // let listener = TcpListener::bind(format!("localhost:{}", cloud_server_parameters.driver_port))
    //     .expect("Failure binding to driver port");
    // for control_stream in listener.incoming() {
    //     match control_stream {
    //         Ok(control_stream) => {
    info!("New benchmark run");
    // let run_parameters = get_cloud_server_run_parameters(control_stream);
    let run_parameters = get_cloud_server_run_parameters_from_json();
    let end_time = calculate_end_time(&run_parameters);
    let thread_handle = thread::spawn(move || {
        execute_new_run(cloud_server_parameters.monitor_port);
    });
    thread::sleep(end_time - Instant::now());
    drop(thread_handle);
    //todo send file to client
    //         }
    //         Err(e) => {
    //             println!("Error: {}", e);
    //             /* connection failed */
    //         }
    //     }
    // }
}

fn get_cloud_server_run_parameters_from_json() -> CloudServerRunParameters {
    serde_json::from_reader(
        File::open("resources/config.json").expect("Could not open config file"),
    )
    .expect("Could not parse MotorDriverParameters from json config file")
}

fn get_cloud_server_run_parameters(mut stream: TcpStream) -> CloudServerRunParameters {
    let mut data = [0; size_of::<CloudServerRunParameters>()];
    let _read = stream
        .read(&mut data)
        .expect("Failure reading data from TcpStream");
    postcard::from_bytes(&data).expect("Failure parsing data into SensorParameters")
}

fn get_cloud_server_parameters(arguments: Vec<String>) -> CloudServerParameters {
    CloudServerParameters {
        driver_port: arguments
            .get(1)
            .expect("Did not receive at least 2 arguments")
            .parse()
            .expect("Could not parse start_port successfully"),
        monitor_port: arguments
            .get(2)
            .expect("Did not receive at least 3 arguments")
            .parse()
            .expect("Could not parse start_port successfully"),
    }
}

fn execute_new_run(monitor_port: u16) {
    let mut alert_protocol = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("alert_protocol.csv")
        .expect("Could not open alert protocol for writing");
    let monitor_listener = TcpListener::bind(format!("localhost:{}", monitor_port))
        .expect("Failure binding to driver port");
    debug!("Bound to localhost:{}", monitor_port);
    for alarm_stream in monitor_listener.incoming() {
        match alarm_stream {
            Ok(mut alarm_stream) => loop {
                if let Some(alert) = get_alert(&mut alarm_stream) {
                    info!("Received monitor message");
                    alert_protocol
                        .write_all(create_alert_csv_line(&alert).as_bytes())
                        .expect("Could not write to alert protocol");
                }
            },
            Err(e) => {
                error!("Error: {}", e);
                /* connection failed */
            }
        }
    }
}

fn create_alert_csv_line(alert: &Alert) -> String {
    format!("{},{},{}\n", alert.time, alert.failure, alert.motor_id,)
}

fn get_alert(alert_stream: &mut TcpStream) -> Option<Alert> {
    let mut raw_buf = [0u8; ALERT_SIZE];
    let mut cobs_buf: CobsAccumulator<ALERT_SIZE> = CobsAccumulator::new();
    let mut alert: Option<Alert> = None;
    debug!("Reading from stream");
    while let Ok(ct) = alert_stream.read(&mut raw_buf) {
        debug!("Read into buffer: {}", ct);
        // Finished reading input
        if ct == 0 {
            break;
        }
        let mut window = &raw_buf[..ct];
        while alert.is_none() && !window.is_empty() {
            debug!("Reading into accumulator");
            window = match cobs_buf.feed::<Alert>(window) {
                FeedResult::Consumed => {
                    debug!("Consumed buffer");
                    break;
                }
                FeedResult::OverFull(new_wind) => {
                    debug!("Overfull");
                    new_wind
                }
                FeedResult::DeserError(new_wind) => {
                    debug!("Deserialization error");
                    new_wind
                }
                FeedResult::Success { data, remaining } => {
                    debug!("Deserialized object");
                    alert = Some(data);
                    remaining
                }
            };
            debug!("Read into accumulator");
        }
        debug!("Read full window {:?}", alert);
        if alert.is_some() {
            return alert;
        }
    }
    debug!("Read");
    alert
}

fn calculate_end_time(cloud_server_run_parameters: &CloudServerRunParameters) -> Instant {
    Instant::now()
        + Duration::from_secs(
            cloud_server_run_parameters
                .start_time
                .try_into()
                .expect("Could not convert time_t start time to i64 start time"),
        )
        + Duration::from_secs(cloud_server_run_parameters.duration)
}
