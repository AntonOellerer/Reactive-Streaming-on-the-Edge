use log::{debug, error, info};
use postcard::to_allocvec;
#[cfg(feature = "rpi")]
use rppal::i2c::I2c;
use serde::Deserialize;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::ops::Shl;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::{fs, thread};
use threadpool::ThreadPool;

use data_transfer_objects::{
    MotorDriverRunParameters, MotorMonitorParameters, RequestProcessingModel, SensorParameters,
};

#[cfg(debug_assertions)]
const CONFIG_PATH: &str = "resources/config-debug.toml";
#[cfg(not(debug_assertions))]
const CONFIG_PATH: &str = "/etc/config-production.toml";

#[derive(Deserialize)]
struct MotorDriverParameters {
    test_driver_listen_address: SocketAddr,
}

fn main() {
    env_logger::init();
    let motor_driver_parameters: MotorDriverParameters =
        toml::from_str(&fs::read_to_string(CONFIG_PATH).expect("Could not read config file"))
            .expect("Could not parse MotorDriverParameters from config file");
    let listener = TcpListener::bind(motor_driver_parameters.test_driver_listen_address)
        .unwrap_or_else(|e| {
            panic!(
                "Could not bind to {}: {e}",
                motor_driver_parameters.test_driver_listen_address
            )
        });
    info!(
        "Bound to {}",
        motor_driver_parameters.test_driver_listen_address
    );
    for test_driver_stream in listener.incoming() {
        info!("Received incoming request");
        match test_driver_stream {
            Ok(mut test_driver_stream) => {
                thread::spawn(move || {
                    info!("New run");
                    let run_parameters =
                        utils::read_object::<MotorDriverRunParameters>(&mut test_driver_stream)
                            .expect("Could not get run parameters");
                    execute_new_run(run_parameters, test_driver_stream);
                    info!("Finished run");
                });
            }
            Err(e) => {
                error!("Error: {}", e);
                /* connection failed */
            }
        }
    }
    info!("Quitting");
}

fn execute_new_run(motor_driver_parameters: MotorDriverRunParameters, test_driver: TcpStream) {
    let motor_monitor_parameters = create_motor_monitor_parameters(&motor_driver_parameters);
    let no_of_sensors = motor_driver_parameters.number_of_tcp_motor_groups * 4;
    let pool = ThreadPool::new(no_of_sensors);
    setup_tcp_sensors(
        motor_driver_parameters.clone(),
        &motor_monitor_parameters,
        &pool,
    );
    #[cfg(feature = "rpi")]
    setup_i2c_sensors(&motor_driver_parameters);
    info!("Setup sensors");
    handle_motor_monitor(
        motor_driver_parameters.request_processing_model,
        motor_monitor_parameters,
        test_driver,
    );
    pool.join();
}

fn setup_tcp_sensors(
    motor_driver_parameters: MotorDriverRunParameters,
    motor_monitor_parameters: &MotorMonitorParameters,
    pool: &ThreadPool,
) {
    let no_i2c = motor_monitor_parameters.number_of_i2c_motor_groups as u16;
    for (index, sensor_driver_address) in motor_driver_parameters
        .sensor_socket_addresses
        .clone()
        .into_iter()
        .enumerate()
    {
        let motor_id = index / 4 + no_i2c as usize;
        let sensor_id = index % 4;
        let full_id: u32 = (motor_id as u32).shl(2) + sensor_id as u32;
        let motor_monitor_listen_address =
            get_motor_monitor_listen_address(motor_monitor_parameters, full_id as u16);
        let sensor_parameters = create_sensor_parameters(
            full_id,
            motor_monitor_listen_address,
            &motor_driver_parameters,
        );
        pool.execute(move || {
            control_sensor(sensor_driver_address, sensor_parameters);
        });
    }
}

fn get_motor_monitor_listen_address(
    motor_monitor_parameters: &MotorMonitorParameters,
    index: u16,
) -> SocketAddr {
    match motor_monitor_parameters.request_processing_model {
        RequestProcessingModel::ReactiveStreaming => motor_monitor_parameters.sensor_listen_address,
        RequestProcessingModel::ClientServer => motor_monitor_parameters.sensor_listen_address,
        RequestProcessingModel::SpringQL => SocketAddr::new(
            motor_monitor_parameters.sensor_listen_address.ip(),
            motor_monitor_parameters.sensor_listen_address.port() + index,
        ),
        RequestProcessingModel::ObjectOriented => SocketAddr::new(
            motor_monitor_parameters.sensor_listen_address.ip(),
            motor_monitor_parameters.sensor_listen_address.port() + index,
        ),
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
                sampling_interval: motor_driver_parameters.sensor_sampling_interval,
                request_processing_model: motor_driver_parameters.request_processing_model,
                motor_monitor_listen_address: SocketAddr::from_str("127.0.0.1:8080").unwrap(), //todo will probably have to deal w/ this separately
                start_time: motor_driver_parameters.start_time,
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
    info!("Running motor monitor");
    let output = create_run_command(request_processing_model)
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
        .arg(motor_monitor_parameters.window_size_ms.to_string())
        .arg(motor_monitor_parameters.sensor_listen_address.to_string())
        .arg(
            motor_monitor_parameters
                .motor_monitor_listen_address
                .to_string(),
        )
        .arg(
            motor_monitor_parameters
                .window_sampling_interval
                .to_string(),
        )
        .arg(
            motor_monitor_parameters
                .sensor_sampling_interval
                .to_string(),
        )
        .arg(motor_monitor_parameters.thread_pool_size.to_string())
        .stderr(Stdio::inherit())
        // .stdout(Stdio::inherit())
        .output()
        .expect("Failure when trying to run motor monitor program");
    info!("Motor monitor run complete");
    stream
        .write_all(&output.stdout)
        .expect("Failure writing sensor stdout to TcpStream");
    info!("Forwarded benchmark data");
}

fn control_sensor(sensor_driver_address: SocketAddr, sensor_parameters: SensorParameters) {
    info!(
        "Sending info to sensor {}, driver address {}, motor monitor listen address {}",
        sensor_parameters.id, sensor_driver_address, sensor_parameters.motor_monitor_listen_address
    );
    match TcpStream::connect(sensor_driver_address) {
        Ok(mut sensor_stream) => {
            write_sensor_parameters(&sensor_parameters, &mut sensor_stream);
        }
        Err(e) => {
            error!("Failed to connect to {sensor_driver_address}: {}", e);
        }
    }
}

#[cfg(debug_assertions)]
fn create_run_command(request_processing_model: RequestProcessingModel) -> Command {
    let dir = match request_processing_model {
        RequestProcessingModel::ReactiveStreaming => "../motor_monitor_rx",
        RequestProcessingModel::ClientServer => "../motor_monitor_cs",
        RequestProcessingModel::SpringQL => "../motor_monitor_sql",
        RequestProcessingModel::ObjectOriented => "../motor_monitor_oo",
    };
    let mut command = Command::new("cargo");
    command.current_dir(dir).arg("run").arg("--");
    command
}

#[cfg(not(debug_assertions))]
fn create_run_command(request_processing_model: RequestProcessingModel) -> Command {
    let command = match request_processing_model {
        RequestProcessingModel::ReactiveStreaming => "motor_monitor_rx",
        RequestProcessingModel::ClientServer => "motor_monitor_cs",
        RequestProcessingModel::SpringQL => "motor_monitor_sql",
        RequestProcessingModel::ObjectOriented => "motor_monitor_oo",
    };
    Command::new(command)
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
        window_size_ms: motor_driver_parameters.window_size_ms,
        sensor_listen_address: motor_driver_parameters.sensor_listen_address,
        motor_monitor_listen_address: motor_driver_parameters.motor_monitor_listen_address,
        sensor_sampling_interval: motor_driver_parameters.sensor_sampling_interval,
        window_sampling_interval: motor_driver_parameters.window_sampling_interval,
        thread_pool_size: motor_driver_parameters.thread_pool_size,
    }
}

fn create_sensor_parameters(
    id: u32,
    motor_monitor_listen_address: SocketAddr,
    motor_driver_parameters: &MotorDriverRunParameters,
) -> SensorParameters {
    SensorParameters {
        id,
        duration: motor_driver_parameters.duration,
        sampling_interval: motor_driver_parameters.sensor_sampling_interval,
        request_processing_model: motor_driver_parameters.request_processing_model,
        motor_monitor_listen_address,
        start_time: motor_driver_parameters.start_time,
    }
}

fn write_sensor_parameters(sensor_parameters: &SensorParameters, stream: &mut TcpStream) {
    let vec: Vec<u8> =
        to_allocvec(&sensor_parameters).expect("Could not write sensor parameters to Vec<u8>");
    stream
        .write_all(&vec)
        .expect("Could not write sensor parameters bytes to TcpStream");
}
