use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::ops::Shl;
use std::process::{Command, Stdio};
use std::time::Duration;
use std::{fs, io, thread};

use log::{error, info};
use postcard::to_allocvec;
#[cfg(feature = "rpi")]
use rppal::i2c::I2c;
use serde::Deserialize;
use threadpool::ThreadPool;

use data_transfer_objects::{
    MotorDriverRunParameters, MotorMonitorParameters, RequestProcessingModel, SensorParameters,
};

#[derive(Deserialize)]
struct MotorDriverParameters {
    test_driver_port: u16,
}

fn main() {
    env_logger::init();
    let motor_driver_parameters: MotorDriverParameters = toml::from_str(
        &fs::read_to_string("resources/config.toml").expect("Could not read config file"),
    )
    .expect("Could not parse MotorDriverParameters from json config file");
    info!(
        "Attempting to bind to port {}",
        motor_driver_parameters.test_driver_port
    );
    let listener = TcpListener::bind(format!(
        "localhost:{}",
        motor_driver_parameters.test_driver_port
    ))
    .expect("Failure binding to port");
    for test_driver_stream in listener.incoming() {
        match test_driver_stream {
            Ok(mut test_driver_stream) => {
                thread::spawn(move || {
                    info!("New run");
                    let run_parameters =
                        utils::read_object::<MotorDriverRunParameters>(&mut test_driver_stream)
                            .expect("Could not get run parameters");
                    execute_new_run(run_parameters, test_driver_stream);
                });
            }
            Err(e) => {
                error!("Error: {}", e);
                /* connection failed */
            }
        }
    }
}

fn execute_new_run(motor_driver_parameters: MotorDriverRunParameters, test_driver: TcpStream) {
    let motor_monitor_parameters = create_motor_monitor_parameters(&motor_driver_parameters);
    let no_of_sensors = motor_driver_parameters.number_of_tcp_motor_groups * 4;
    let pool = ThreadPool::new(no_of_sensors);
    setup_tcp_sensors(
        &motor_driver_parameters,
        &test_driver,
        &motor_monitor_parameters,
        &pool,
    );
    #[cfg(feature = "rpi")]
    setup_i2c_sensors(&motor_driver_parameters);
    handle_motor_monitor(
        motor_driver_parameters.request_processing_model,
        motor_monitor_parameters,
        test_driver,
    );
    pool.join();
}

fn setup_tcp_sensors(
    motor_driver_parameters: &MotorDriverRunParameters,
    test_driver: &TcpStream,
    motor_monitor_parameters: &MotorMonitorParameters,
    pool: &ThreadPool,
) {
    let no_i2c = motor_monitor_parameters.number_of_i2c_motor_groups as u16;
    for motor_id in no_i2c..motor_driver_parameters.number_of_tcp_motor_groups as u16 + no_i2c {
        for sensor_id in 0..4u16 {
            let motor_driver_parameters_cloned = *motor_driver_parameters;
            let full_id: u32 = (motor_id as u32).shl(16) + sensor_id as u32;
            //fixme shouldn't the multiplicator be 4?
            let driver_port: u16 = motor_driver_parameters.sensor_driver_start_port
                + (motor_id - no_i2c) * 5
                + sensor_id;
            let sensor_port: u16 = motor_monitor_parameters.sensor_port;
            let mut test_driver_stream_copy =
                test_driver.try_clone().expect("Could not clone stream");
            pool.execute(move || {
                control_sensor(
                    full_id,
                    driver_port,
                    sensor_port,
                    &motor_driver_parameters_cloned,
                    &mut test_driver_stream_copy,
                );
            });
        }
    }
}

#[cfg(feature = "rpi")]
fn setup_i2c_sensors(motor_driver_parameters: &MotorDriverRunParameters) {
    let mut message_buffer = [0u8; 32];
    let mut i2c = I2c::new().expect("Could not instantiate i2c object");
    for motor_id in 0..motor_driver_parameters.number_of_i2c_motor_groups {
        for sensor_no in 0..4u8 {
            let sensor_id: u8 = (motor_id).shl(2) + sensor_no;
            let parameters = SensorParameters {
                id: sensor_id as u32,
                duration: motor_driver_parameters.duration,
                sampling_interval: motor_driver_parameters.sampling_interval,
                request_processing_model: motor_driver_parameters.request_processing_model,
                motor_monitor_port: 0,
            };
            let message = postcard::to_slice_cobs(&parameters, &mut message_buffer)
                .expect("Could not write i2c sensor parameters to slice");
            i2c.set_slave_address(sensor_id as u16)
                .unwrap_or_else(|_| panic!("Could not set slave address to {sensor_id}"));
            i2c.write(message)
                .expect("Could not write sensor parameters to i2c");
        }
    }
}

fn handle_motor_monitor(
    request_processing_model: RequestProcessingModel,
    motor_monitor_parameters: MotorMonitorParameters,
    mut stream: TcpStream,
) {
    println!("Running motor monitor");
    let dir = match request_processing_model {
        RequestProcessingModel::ReactiveStreaming => "../motor_monitor_rx",
        RequestProcessingModel::ClientServer => "../motor_monitor_cs",
    };
    let output = Command::new("cargo")
        .current_dir(dir)
        .arg("run")
        .arg("--")
        .arg(motor_monitor_parameters.start_time.to_string())
        .arg(motor_monitor_parameters.duration.to_string())
        .arg(request_processing_model.to_string())
        .arg(
            motor_monitor_parameters
                .number_of_tcp_motor_groups
                .to_string(),
        )
        .arg(
            motor_monitor_parameters
                .number_of_i2c_motor_groups
                .to_string(),
        )
        .arg(motor_monitor_parameters.window_size.to_string())
        .arg(motor_monitor_parameters.sensor_port.to_string())
        .arg(motor_monitor_parameters.cloud_server_port.to_string())
        .stderr(Stdio::inherit())
        // .stdout(Stdio::inherit())
        .output()
        .expect("Failure when trying to run motor monitor program");
    stream
        .write_all(&output.stdout)
        .expect("Failure writing sensor stdout to TcpStream");
}

fn control_sensor(
    id: u32,
    driver_port: u16,
    sensor_port: u16,
    motor_driver_parameters: &MotorDriverRunParameters,
    test_driver_stream: &mut TcpStream,
) {
    info!(
        "Sending info to sensor {}, driver port {}, sensor port {}",
        id, driver_port, sensor_port
    );
    let sensor_parameters = create_sensor_parameters(id, sensor_port, motor_driver_parameters);
    match TcpStream::connect(format!("localhost:{driver_port}")) {
        Ok(mut sensor_stream) => {
            write_sensor_parameters(&sensor_parameters, &mut sensor_stream);
            thread::sleep(
                utils::get_duration_to_end(
                    Duration::from_secs_f64(motor_driver_parameters.start_time),
                    Duration::from_secs_f64(motor_driver_parameters.duration),
                ) + Duration::from_secs(10),
            );
            info!("Copying data {}", id);
            copy_sensor_benchmark_data(&mut sensor_stream, test_driver_stream);
        }
        Err(e) => {
            error!("Failed to connect: {}", e);
        }
    }
}

fn create_motor_monitor_parameters(
    motor_driver_parameters: &MotorDriverRunParameters,
) -> MotorMonitorParameters {
    MotorMonitorParameters {
        start_time: motor_driver_parameters.start_time,
        duration: motor_driver_parameters.duration,
        request_processing_model: motor_driver_parameters.request_processing_model,
        number_of_tcp_motor_groups: motor_driver_parameters.number_of_tcp_motor_groups,
        number_of_i2c_motor_groups: motor_driver_parameters.number_of_i2c_motor_groups,
        window_size: motor_driver_parameters.window_size_seconds * 1000_f64
            / motor_driver_parameters.sampling_interval as f64,
        sensor_port: motor_driver_parameters.sensor_port,
        cloud_server_port: motor_driver_parameters.cloud_server_port,
    }
}

fn create_sensor_parameters(
    id: u32,
    port: u16,
    motor_driver_parameters: &MotorDriverRunParameters,
) -> SensorParameters {
    SensorParameters {
        id,
        duration: motor_driver_parameters.duration,
        sampling_interval: motor_driver_parameters.sampling_interval,
        request_processing_model: RequestProcessingModel::ClientServer,
        motor_monitor_port: port,
    }
}

fn write_sensor_parameters(sensor_parameters: &SensorParameters, stream: &mut TcpStream) {
    let vec: Vec<u8> =
        to_allocvec(&sensor_parameters).expect("Could not write sensor parameters to Vec<u8>");
    stream
        .write_all(&vec)
        .expect("Could not write sensor parameters bytes to TcpStream");
}

fn copy_sensor_benchmark_data(
    sensor_driver_stream: &mut TcpStream,
    test_driver_stream: &mut TcpStream,
) {
    io::copy(sensor_driver_stream, test_driver_stream)
        .expect("Could not read/write from sensor driver to test driver");
}
