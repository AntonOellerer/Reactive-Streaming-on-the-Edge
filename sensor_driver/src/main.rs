use log::{error, info};
use std::io::Read;
use std::mem::size_of;
use std::net::{TcpListener, TcpStream};
use std::ops::BitAnd;
use std::process::{Command, Stdio};
use std::thread;

use data_transfer_objects::SensorParameters;

#[cfg(debug_assertions)]
const RESOURCE_PATH: &str = "resources";
#[cfg(not(debug_assertions))]
const RESOURCE_PATH: &str = "/etc";

fn main() {
    env_logger::init();
    let listener_address = std::env::args().nth(1).expect("no listener address given");
    let listener = TcpListener::bind(listener_address.clone())
        .unwrap_or_else(|e| panic!("Could not bind to {listener_address}: {e}"));
    info!("Bound to {listener_address}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || {
                    info!("New connection");
                    start_new_run(stream);
                    info!("Finished benchmark run");
                });
            }
            Err(e) => {
                error!("Error: {e}");
                /* connection failed */
            }
        }
    }
}

fn start_new_run(mut stream: TcpStream) {
    let mut data = [0; size_of::<SensorParameters>()];
    let _read = stream
        .read(&mut data)
        .expect("Failure reading data from TcpStream");
    let sensor_parameters: SensorParameters =
        postcard::from_bytes(&data).expect("Failure parsing data into SensorParameters");
    info!(
        "Running sensor {}, motor monitor listen address {}",
        sensor_parameters.id, sensor_parameters.motor_monitor_listen_address
    );
    create_run_command()
        .arg(format!(
            "{}/{}.txt",
            RESOURCE_PATH,
            sensor_parameters.id.bitand(0x0003)
        ))
        .arg(sensor_parameters.id.to_string())
        .arg(sensor_parameters.duration.to_string())
        .arg(sensor_parameters.sampling_interval.to_string())
        .arg(sensor_parameters.request_processing_model.to_string())
        .arg(sensor_parameters.motor_monitor_listen_address.to_string())
        .arg(sensor_parameters.start_time.to_string())
        .stderr(Stdio::inherit())
        .output()
        .expect("Failure when trying to run sensor program");
}

#[cfg(debug_assertions)]
fn create_run_command() -> Command {
    let mut command = Command::new("cargo");
    command.current_dir("../sensor").arg("run").arg("--");
    command
}

#[cfg(not(debug_assertions))]
fn create_run_command() -> Command {
    Command::new("sensor")
}
