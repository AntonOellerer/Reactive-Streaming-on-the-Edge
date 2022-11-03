use std::io::Write;
use std::mem::size_of;
use std::net::{TcpListener, TcpStream};
#[cfg(feature = "rpi")]
use std::ops::Shl;
use std::ops::{Add, BitAnd, Shr};
use std::str::FromStr;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use libc::time_t;
use postcard::to_allocvec_cobs;
use procfs::process::Process;
#[cfg(feature = "rpi")]
use rppal::i2c::I2c;

use threadpool::ThreadPool;

use data_transfer_objects::{
    Alert, BenchmarkData, BenchmarkDataType, MotorFailure, MotorMonitorParameters,
    RequestProcessingModel, SensorMessage,
};

use crate::motor_sensor_group_buffers::MotorGroupSensorsBuffers;
use crate::sliding_window::SlidingWindow;

mod motor_sensor_group_buffers;
mod rules_engine;
mod sliding_window;

#[derive(Debug)]
pub struct TimedSensorMessage {
    pub timestamp: time_t,
    reading: f32,
    _sensor_id: u32,
}

impl From<SensorMessage> for TimedSensorMessage {
    fn from(sensor_message: SensorMessage) -> Self {
        TimedSensorMessage {
            timestamp: utils::get_now(),
            reading: sensor_message.reading,
            _sensor_id: sensor_message.sensor_id,
        }
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let motor_monitor_parameters: MotorMonitorParameters = get_motor_monitor_parameters(&arguments);
    execute_client_server_procedure(&motor_monitor_parameters);
}

fn execute_client_server_procedure(motor_monitor_parameters: &MotorMonitorParameters) {
    let sleep_duration = utils::get_duration_to_end(
        motor_monitor_parameters.start_time,
        motor_monitor_parameters.duration,
    )
    .add(Duration::from_secs(1)); //to account for all sensor messages
    let (tx, rx) = channel();
    let mmpc = *motor_monitor_parameters;
    let sensor_listener = thread::spawn(move || setup_sensor_handlers(mmpc, tx));
    let consumer_thread = handle_consumer(rx, motor_monitor_parameters);
    thread::sleep(sleep_duration);
    save_benchmark_readings();
    drop(sensor_listener);
    drop(consumer_thread);
}

fn get_motor_monitor_parameters(arguments: &[String]) -> MotorMonitorParameters {
    MotorMonitorParameters {
        start_time: arguments
            .get(1)
            .expect("Did not receive at least 2 arguments")
            .parse()
            .expect("Could not parse start_time successfully"),
        duration: arguments
            .get(2)
            .expect("Did not receive at least 3 arguments")
            .parse()
            .expect("Could not parse duration successfully"),
        request_processing_model: RequestProcessingModel::from_str(
            arguments
                .get(3)
                .expect("Did not receive at least 4 arguments"),
        )
        .expect("Could not parse Request Processing Model successfully"),
        number_of_tcp_motor_groups: arguments
            .get(4)
            .expect("Did not receive at least 5 arguments")
            .parse()
            .expect("Could not parse number_of_motor_groups successfully"),
        number_of_i2c_motor_groups: arguments
            .get(5)
            .expect("Did not receive at least 5 arguments")
            .parse()
            .expect("Could not parse number_of_motor_groups successfully"),
        window_size: arguments
            .get(6)
            .expect("Did not receive at least 6 arguments")
            .parse()
            .expect("Could not parse window_size successfully"),
        sensor_port: arguments
            .get(7)
            .expect("Did not receive at least 7 arguments")
            .parse()
            .expect("Could not parse start_port successfully"),
        cloud_server_port: arguments
            .get(8)
            .expect("Did not receive at least 8 arguments")
            .parse()
            .expect("Could not parse cloud_server_port successfully"),
    }
}

fn setup_sensor_handlers(args: MotorMonitorParameters, tx: Sender<SensorMessage>) {
    let pool = ThreadPool::new(8); //TODO
    setup_tcp_sensor_handlers(&args, tx.clone(), &pool);
    #[cfg(feature = "rpi")]
    setup_i2c_sensor_handlers(&args, tx, &pool);
}

fn setup_tcp_sensor_handlers(
    args: &MotorMonitorParameters,
    tx: Sender<SensorMessage>,
    pool: &ThreadPool,
) {
    let port = args.sensor_port;
    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .unwrap_or_else(|_| panic!("Could not bind sensor data listener to {port}"));
    for stream in listener.incoming() {
        let tx = tx.clone();
        pool.execute(move || {
            match stream {
                Ok(mut stream) => loop {
                    handle_sensor_message(&tx, &mut stream);
                },
                Err(e) => {
                    eprintln!("Error: {e}");
                    /* connection failed */
                }
            }
        });
    }
}

#[cfg(feature = "rpi")]
fn setup_i2c_sensor_handlers(
    args: &MotorMonitorParameters,
    tx: Sender<SensorMessage>,
    pool: &ThreadPool,
) {
    let mut i2c = I2c::new().expect("Could not instantiate i2c object");
    let number_of_motor_groups = args.number_of_i2c_motor_groups;
    pool.execute(move || {
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
    });
}

fn handle_sensor_message(tx: &Sender<SensorMessage>, stream: &mut TcpStream) {
    if let Some(message) = utils::read_object::<SensorMessage>(stream) {
        tx.send(message)
            .unwrap_or_else(|_| eprintln!("Could not parse sensor message successfully"))
    }
}

fn handle_consumer(
    rx: Receiver<SensorMessage>,
    motor_monitor_parameters: &MotorMonitorParameters,
) -> JoinHandle<()> {
    let mut cloud_server = TcpStream::connect(format!(
        "localhost:{}",
        motor_monitor_parameters.cloud_server_port
    ))
    .expect("Could not open connection to cloud server");
    eprintln!(
        "Connected to localhost:{}",
        motor_monitor_parameters.cloud_server_port
    );
    let motor_monitor_parameters = *motor_monitor_parameters;
    thread::spawn(move || {
        let total_motors = motor_monitor_parameters.number_of_tcp_motor_groups
            + motor_monitor_parameters.number_of_i2c_motor_groups as usize;
        let mut buffers: Vec<MotorGroupSensorsBuffers> = Vec::with_capacity(total_motors);
        for _ in 0..total_motors {
            buffers.push(MotorGroupSensorsBuffers::new(
                motor_monitor_parameters.window_size,
            ))
        }
        let end_time =
            motor_monitor_parameters.start_time + motor_monitor_parameters.duration as time_t;
        while utils::get_now() < end_time {
            let message = rx.recv();
            match message {
                Ok(message) => {
                    handle_message(&mut buffers, message, &mut cloud_server);
                }
                Err(e) => eprintln!("Error: {e}"),
            };
        }
    })
}

fn handle_message(
    buffers: &mut [MotorGroupSensorsBuffers],
    message: SensorMessage,
    cloud_server: &mut TcpStream,
) {
    let motor_group_id: u32 = message.sensor_id.shr(u32::BITS / 2);
    let sensor_id = message.sensor_id.bitand(0xFFFF);
    let motor_group_buffers = get_motor_group_buffers(buffers, motor_group_id);
    add_message_to_sensor_buffer(message, sensor_id, motor_group_buffers);
    let now = utils::get_now();
    motor_group_buffers.refresh_caches(now);
    let rule_violated = rules_engine::violated_rule(motor_group_buffers);
    if let Some(rule) = rule_violated {
        eprintln!("Found rule violation {rule} in motor {motor_group_id}");
        let alert = create_alert(motor_group_id, now, rule);
        let vec: Vec<u8> =
            to_allocvec_cobs(&alert).expect("Could not write motor monitor alert to Vec<u8>");
        cloud_server
            .write_all(&vec)
            .expect("Could not send motor alert to cloud server");
        motor_group_buffers.reset();
    }
}

fn add_message_to_sensor_buffer(
    message: SensorMessage,
    sensor_id: u32,
    motor_group: &mut MotorGroupSensorsBuffers,
) {
    let sensor_buffer =
        &mut motor_group[usize::try_from(sensor_id).expect("Could not convert u32 id to usize")];
    sensor_buffer.add(TimedSensorMessage::from(message));
}

fn get_motor_group_buffers(
    buffers: &mut [MotorGroupSensorsBuffers],
    motor_group_id: u32,
) -> &mut MotorGroupSensorsBuffers {
    buffers
        .get_mut(usize::try_from(motor_group_id).expect("Could not convert u32 id to usize"))
        .expect("Motor group id did not match to a motor group buffer")
}

fn create_alert(motor_group_id: u32, now: time_t, rule: MotorFailure) -> Alert {
    Alert {
        time: now,
        motor_id: motor_group_id as u16,
        failure: rule,
    }
}

fn save_benchmark_readings() {
    let me = Process::myself().expect("Could not get process info handle");
    let stat = me.stat().expect("Could not get /proc/[pid]/stat info");
    let status = me.status().expect("Could not get /proc/[pid]/status info");
    let benchmark_data = BenchmarkData {
        id: 0,
        time_spent_in_kernel_mode: stat.stime,
        time_spent_in_user_mode: stat.utime,
        children_time_spent_in_kernel_mode: stat.cstime,
        children_time_spent_in_user_mode: stat.cutime,
        memory_high_water_mark: status.vmhwm.expect("Could not get vmhw"),
        memory_resident_set_size: status.vmrss.expect("Could not get vmrss"),
        benchmark_data_type: BenchmarkDataType::MotorMonitor,
    };
    let vec: Vec<u8> =
        to_allocvec_cobs(&benchmark_data).expect("Could not write benchmark data to Vec<u8>");
    let _ = std::io::stdout()
        .write(&vec)
        .expect("Could not write benchmark data bytes to stdout");
}
