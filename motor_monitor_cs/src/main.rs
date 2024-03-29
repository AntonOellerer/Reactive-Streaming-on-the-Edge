use crate::motor_sensor_group_buffers::MotorGroupSensorsBuffers;
use crate::sliding_window::SlidingWindow;
use data_transfer_objects::{
    Alert, BenchmarkDataType, MotorFailure, MotorMonitorParameters, SensorMessage,
};
use env_logger::Target;
use futures::executor::{ThreadPool, ThreadPoolBuilder};
use futures::future::RemoteHandle;
use log::{debug, error, info};
use postcard::to_allocvec_cobs;
#[cfg(feature = "rpi")]
use rppal::i2c::I2c;
use scheduler::Scheduler;
use std::io::Write;
#[cfg(feature = "rpi")]
use std::mem::size_of;
use std::net::{TcpListener, TcpStream};
#[cfg(feature = "rpi")]
use std::ops::Shl;
use std::ops::{BitAnd, Shr};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

mod motor_sensor_group_buffers;
mod rules_engine;
mod sliding_window;

fn main() {
    env_logger::builder().target(Target::Stderr).init();
    let arguments: Vec<String> = std::env::args().collect();
    let motor_monitor_parameters: MotorMonitorParameters =
        utils::get_motor_monitor_parameters(&arguments);
    execute_client_server_procedure(&motor_monitor_parameters);
}

fn execute_client_server_procedure(motor_monitor_parameters: &MotorMonitorParameters) {
    let (tx, rx) = channel();
    let pool = ThreadPoolBuilder::new()
        .pool_size(motor_monitor_parameters.thread_pool_size)
        .create()
        .unwrap();
    let mut handle_list = handle_sensors(*motor_monitor_parameters, tx, &pool);
    info!("Setup complete");
    handle_list.push(handle_consumer(rx, motor_monitor_parameters, &pool));
    wait_on_complete(handle_list);
    info!("Processing completed");
    utils::save_benchmark_readings(0, BenchmarkDataType::MotorMonitor);
    info!("Saved benchmark readings");
}

fn wait_on_complete(handle_list: Vec<RemoteHandle<()>>) {
    for handle in handle_list {
        futures::executor::block_on(handle);
    }
}

fn handle_sensors(
    args: MotorMonitorParameters,
    tx: Sender<SensorMessage>,
    pool: &ThreadPool,
) -> Vec<RemoteHandle<()>> {
    setup_tcp_sensor_handlers(&args, tx.clone(), pool)
}

fn setup_tcp_sensor_handlers(
    motor_monitor_parameters: &MotorMonitorParameters,
    tx: Sender<SensorMessage>,
    pool: &ThreadPool,
) -> Vec<RemoteHandle<()>> {
    info!(
        "Listening on 0.0.0.0:{}",
        motor_monitor_parameters.sensor_listen_address.port()
    );
    let listener = TcpListener::bind(format!(
        "0.0.0.0:{}",
        motor_monitor_parameters.sensor_listen_address.port()
    ))
    .unwrap_or_else(|e| {
        panic!(
            "Could not bind sensor data listener to {}: {e}",
            motor_monitor_parameters.sensor_listen_address
        )
    });
    info!(
        "Bound listener on sensor listener address {}",
        motor_monitor_parameters.sensor_listen_address
    );
    let total_number_of_motors = motor_monitor_parameters.number_of_tcp_motor_groups
        + motor_monitor_parameters.number_of_i2c_motor_groups as usize;
    let total_number_of_sensors = total_number_of_motors * 4;
    let mut handle_list = vec![];
    for _ in 0..total_number_of_sensors {
        let tx = tx.clone();
        let stream = listener.accept();
        let handle = pool.schedule(move || {
            match stream {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .expect("Could not set read timeout");
                    while let Some(sensor_message) =
                        utils::read_object::<SensorMessage>(&mut stream)
                    {
                        handle_sensor_message(sensor_message, &tx);
                    }
                }
                Err(e) => {
                    error!("Error: {e}");
                    /* connection failed */
                }
            }
        });
        handle_list.push(handle);
    }
    handle_list
}

#[cfg(feature = "rpi")]
fn setup_i2c_sensor_handlers(
    args: &MotorMonitorParameters,
    tx: Sender<SensorMessage>,
    pool: &ThreadPool,
) -> RemoteHandle<()> {
    let mut i2c = I2c::new().expect("Could not instantiate i2c object");
    let number_of_motor_groups = args.number_of_i2c_motor_groups;
    pool.schedule(move || {
        let mut data = [0u8; size_of::<SensorMessage>()];
        loop {
            for motor_id in 0..number_of_motor_groups {
                for sensor_no in 0..4u8 {
                    let sensor_id: u8 = (motor_id).shl(2) + sensor_no;
                    i2c.set_slave_address(sensor_id as u16)
                        .unwrap_or_else(|_| panic!("Could not set sensor address to {sensor_id}"));
                    let read_amount = i2c
                        .read(&mut data)
                        .unwrap_or_else(|_| panic!("Failed to read from i2c sensor {sensor_id}"));
                    if read_amount > 0 {
                        let message = postcard::from_bytes_cobs::<SensorMessage>(&mut data)
                            .expect("Could not parse sensor message to struct");
                        tx.send(message).expect("Could not forward sensor message");
                    }
                }
            }
        }
    })
}

fn handle_sensor_message(message: SensorMessage, tx: &Sender<SensorMessage>) {
    debug!("{message:?}");
    tx.send(message)
        .expect("Could not send sensor message to handler");
}

fn handle_consumer(
    rx: Receiver<SensorMessage>,
    motor_monitor_parameters: &MotorMonitorParameters,
    pool: &ThreadPool,
) -> RemoteHandle<()> {
    let mut cloud_server =
        TcpStream::connect(motor_monitor_parameters.motor_monitor_listen_address)
            .expect("Could not open connection to cloud server");
    info!(
        "Connected to {}",
        motor_monitor_parameters.motor_monitor_listen_address
    );
    let motor_monitor_parameters = *motor_monitor_parameters;
    pool.schedule(move || {
        let total_motors = motor_monitor_parameters.number_of_tcp_motor_groups
            + motor_monitor_parameters.number_of_i2c_motor_groups as usize;
        let mut buffers: Vec<MotorGroupSensorsBuffers> = Vec::with_capacity(total_motors);
        for _ in 0..total_motors {
            buffers.push(MotorGroupSensorsBuffers::new(Duration::from_millis(
                motor_monitor_parameters.window_size_ms
                    / motor_monitor_parameters.sensor_sampling_interval as u64,
            )))
        }
        while let Ok(message) = rx.recv() {
            handle_message(&mut buffers, message, &mut cloud_server);
        }
    })
}

fn handle_message(
    buffers: &mut [MotorGroupSensorsBuffers],
    message: SensorMessage,
    cloud_server: &mut TcpStream,
) {
    let motor_group_id: u32 = message.sensor_id.shr(2);
    let sensor_id = message.sensor_id.bitand(0x0003);
    let motor_group_buffers = get_motor_group_buffers(buffers, motor_group_id);
    add_message_to_sensor_buffer(message, sensor_id, motor_group_buffers);
    motor_group_buffers.refresh_caches(Duration::from_secs_f64(message.timestamp));
    if motor_group_buffers.is_some() {
        let rule_violated = rules_engine::violated_rule(motor_group_buffers);
        if let Some(failure) = rule_violated {
            info!("{motor_group_buffers:?}");
            info!("Found rule violation {failure} in motor {motor_group_id}");
            let alert = create_alert(motor_group_id, motor_group_buffers.get_time(), failure);
            let vec: Vec<u8> =
                to_allocvec_cobs(&alert).expect("Could not write motor monitor alert to Vec<u8>");
            cloud_server
                .write_all(&vec)
                .expect("Could not send motor alert to cloud server");
            motor_group_buffers.reset();
        }
    }
}

fn add_message_to_sensor_buffer(
    message: SensorMessage,
    sensor_id: u32,
    motor_group: &mut MotorGroupSensorsBuffers,
) {
    let sensor_buffer =
        &mut motor_group[usize::try_from(sensor_id).expect("Could not convert u32 id to usize")];
    sensor_buffer.add(message);
}

fn get_motor_group_buffers(
    buffers: &mut [MotorGroupSensorsBuffers],
    motor_group_id: u32,
) -> &mut MotorGroupSensorsBuffers {
    buffers
        .get_mut(usize::try_from(motor_group_id).expect("Could not convert u32 id to usize"))
        .expect("Motor group id did not match to a motor group buffer")
}

fn create_alert(motor_group_id: u32, time: f64, failure: MotorFailure) -> Alert {
    Alert {
        time,
        motor_id: motor_group_id as u16,
        failure,
    }
}
