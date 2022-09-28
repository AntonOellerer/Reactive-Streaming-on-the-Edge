use std::io::{Read, Write};
use std::mem::size_of;
use std::net::{TcpListener, TcpStream};
use std::ops::BitAnd;
use std::process::{Command, Stdio};
use std::thread;

use data_transfer_objects::SensorParameters;

fn main() {
    let driver_port = std::env::args().nth(1).expect("no port given");
    let listener =
        TcpListener::bind(format!("localhost:{}", driver_port)).expect("Failure binding to port");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || {
                    println!("New connection");
                    start_new_run(stream);
                });
            }
            Err(e) => {
                println!("Error: {}", e);
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
    println!("Running sensor");
    let output = Command::new("cargo")
        .current_dir("../sensor")
        .arg("run")
        .arg("--")
        .arg(format!(
            "resources/{}.txt",
            sensor_parameters.id.bitand(0xFFFF)
        ))
        .arg(sensor_parameters.id.to_string())
        .arg(sensor_parameters.start_time.to_string())
        .arg(sensor_parameters.duration.to_string())
        .arg(sensor_parameters.seed.to_string())
        .arg(sensor_parameters.sampling_interval.to_string())
        .arg(sensor_parameters.request_processing_model.to_string())
        .arg(sensor_parameters.port.to_string())
        .stderr(Stdio::inherit())
        .output()
        .expect("Failure when trying to run sensor program");
    stream
        .write_all(&output.stdout)
        .expect("Failure writing sensor stdout to TcpStream");
}
