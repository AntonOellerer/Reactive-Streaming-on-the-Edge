use std::fs::OpenOptions;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;
use std::{fs, thread};

use log::{error, info};
use serde::Deserialize;

use data_transfer_objects::{Alert, CloudServerRunParameters};

#[cfg(debug_assertions)]
const CONFIG_PATH: &str = "resources/config-debug.toml";
#[cfg(not(debug_assertions))]
const CONFIG_PATH: &str = "/etc/config-production.toml";

#[derive(Deserialize)]
struct CloudServerParameters {
    test_driver_listen_address: SocketAddr,
}

fn main() {
    env_logger::init();
    let cloud_server_parameters: CloudServerParameters =
        toml::from_str(&fs::read_to_string(CONFIG_PATH).expect("Could not read config file"))
            .expect("Could not parse config file");
    let listener = TcpListener::bind(cloud_server_parameters.test_driver_listen_address)
        .unwrap_or_else(|_| {
            panic!(
                "Failure binding to listener address {}",
                cloud_server_parameters.test_driver_listen_address
            )
        });
    info!(
        "Listening on {}",
        cloud_server_parameters.test_driver_listen_address
    );
    for control_stream in listener.incoming() {
        match control_stream {
            Ok(mut control_stream) => {
                info!("New run");
                let run_parameters =
                    utils::read_object::<CloudServerRunParameters>(&mut control_stream)
                        .expect("Could not get run parameters");
                let thread_handle = thread::spawn(move || {
                    execute_new_run(run_parameters.motor_monitor_listen_address);
                });
                thread::sleep(utils::get_duration_to_end(
                    Duration::from_secs_f64(run_parameters.start_time),
                    Duration::from_secs_f64(run_parameters.duration),
                ));
                info!("Dropping handle");
                drop(thread_handle);
                send_alerts_to_driver(&mut control_stream);
            }
            Err(e) => {
                error!("Error: {}", e);
                /* connection failed */
            }
        }
    }
}

fn send_alerts_to_driver(control_stream: &mut TcpStream) {
    control_stream
        .write_all(&fs::read("alert_protocol.csv").expect("Could not get alert file bytes"))
        .expect("Could not send alert file to test driver");
}

fn execute_new_run(monitor_listen_address: SocketAddr) {
    let mut alert_protocol = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("alert_protocol.csv")
        .expect("Could not open alert protocol for writing");
    info!("Binding to {monitor_listen_address}");
    let monitor_listener = TcpListener::bind(monitor_listen_address).unwrap();
    let alarm_stream = monitor_listener.accept();
    match alarm_stream {
        Ok((mut alarm_stream, _)) => {
            while let Some(alert) = utils::read_object::<Alert>(&mut alarm_stream) {
                let delay = utils::get_now_duration() - Duration::from_secs_f64(alert.time);
                info!("Received monitor message, delay: {delay:?}");
                writeln!(alert_protocol, "{},{}", alert.to_csv(), delay.as_secs_f64())
                    .expect("Could not write to alert protocol");
            }
        }
        Err(e) => {
            error!("Error: {}", e);
            /* connection failed */
        }
    }
}
