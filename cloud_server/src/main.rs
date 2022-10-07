use std::{fs, thread};
use std::fs::OpenOptions;
use std::io::Write;
use std::net::TcpListener;

use log::{error, info};
use serde::Deserialize;

use data_transfer_objects::{Alert, CloudServerRunParameters};

#[derive(Deserialize)]
struct CloudServerParameters {
    test_driver_port: u16,
}

fn main() {
    env_logger::init();
    let cloud_server_parameters: CloudServerParameters = toml::from_str(
        &fs::read_to_string("resources/config.toml").expect("Could not read config file"),
    )
        .expect("Could not parse config file");
    let listener = TcpListener::bind(format!(
        "localhost:{}",
        cloud_server_parameters.test_driver_port
    ))
        .unwrap_or_else(|_| {
            panic!(
                "Failure binding to driver port {}",
                cloud_server_parameters.test_driver_port
            )
        });
    for control_stream in listener.incoming() {
        match control_stream {
            Ok(mut control_stream) => {
                info!("New run");
                let run_parameters =
                    utils::read_object::<CloudServerRunParameters>(&mut control_stream)
                        .expect("Could not get run parameters");
                let thread_handle = thread::spawn(move || {
                    execute_new_run(run_parameters.motor_monitor_port);
                });
                thread::sleep(utils::get_sleep_duration(
                    run_parameters.start_time,
                    run_parameters.duration,
                ));
                info!("Dropping handle");
                drop(thread_handle);
                //todo send file to test driver
            }
            Err(e) => {
                error!("Error: {}", e);
                /* connection failed */
            }
        }
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
        .unwrap_or_else(|_| panic!("Failure binding to monitor port {}", monitor_port));
    info!("Bound to localhost:{}", monitor_port);
    let alarm_stream = monitor_listener.accept();
    match alarm_stream {
        Ok((mut alarm_stream, _)) => {
            while let Some(alert) = utils::read_object::<Alert>(&mut alarm_stream) {
                info!("Received monitor message");
                alert_protocol
                    .write_all(alert.to_csv_string().as_bytes())
                    .expect("Could not write to alert protocol");
            }
        }
        Err(e) => {
            error!("Error: {}", e);
            /* connection failed */
        }
    }
}
