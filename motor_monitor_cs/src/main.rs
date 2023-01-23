use futures::executor::{ThreadPool, ThreadPoolBuilder};
use futures::future::RemoteHandle;

use std::io::Write;
#[cfg(feature = "rpi")]
use std::mem::size_of;
use std::net::{TcpListener, TcpStream};
#[cfg(feature = "rpi")]
use std::ops::Shl;
use std::ops::{BitAnd, Shr};
use std::str::FromStr;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use postcard::to_allocvec_cobs;
use procfs::process::Process;
#[cfg(feature = "rpi")]
use rppal::i2c::I2c;
use rx_rust_mp::scheduler::Scheduler;

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
    pub timestamp: f64,
    reading: f32,
    _sensor_id: u32,
}

impl From<SensorMessage> for TimedSensorMessage {
    fn from(sensor_message: SensorMessage) -> Self {
        TimedSensorMessage {
            timestamp: utils::get_now_secs(),
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
    let (tx, rx) = channel();
    let pool = ThreadPoolBuilder::new()
        .pool_size(motor_monitor_parameters.thread_pool_size)
        .create()
        .unwrap();
    let mut handle_list = handle_sensors(*motor_monitor_parameters, tx, &pool);
    handle_list.push(handle_consumer(rx, motor_monitor_parameters, &pool));
    wait_on_complete(handle_list);
    save_benchmark_readings();
}

fn wait_on_complete(handle_list: Vec<RemoteHandle<()>>) {
    for handle in handle_list {
        futures::executor::block_on(handle);
    }
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
        sampling_interval: arguments
            .get(9)
            .expect("Did not receive at least 9 arguments")
            .parse()
            .expect("Could not parse sampling_interval successfully"),
        thread_pool_size: arguments
            .get(10)
            .expect("Did not receive at least 10 arguments")
            .parse()
            .expect("Could not parse thread_pool_size successfully"),
    }
}

fn handle_sensors(
    args: MotorMonitorParameters,
    tx: Sender<SensorMessage>,
    pool: &ThreadPool,
) -> Vec<RemoteHandle<()>> {
    let mut handle_list = setup_tcp_sensor_handlers(&args, tx.clone(), pool);
    #[cfg(feature = "rpi")]
    handle_list.push(setup_i2c_sensor_handlers(&args, tx, pool));
    handle_list
}

fn setup_tcp_sensor_handlers(
    motor_monitor_parameters: &MotorMonitorParameters,
    tx: Sender<SensorMessage>,
    pool: &ThreadPool,
) -> Vec<RemoteHandle<()>> {
    let port = motor_monitor_parameters.sensor_port;
    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .unwrap_or_else(|_| panic!("Could not bind sensor data listener to {port}"));
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
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .expect("Could not set read timeout");
                    while let Some(sensor_message) =
                        utils::read_object::<SensorMessage>(&mut stream)
                    {
                        handle_sensor_message(sensor_message, &tx);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
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
    eprintln!("{message:?}");
    tx.send(message)
        .unwrap_or_else(|e| eprintln!("Could not send sensor message successfully {e}"))
}

fn handle_consumer(
    rx: Receiver<SensorMessage>,
    motor_monitor_parameters: &MotorMonitorParameters,
    pool: &ThreadPool,
) -> RemoteHandle<()> {
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
    pool.schedule(move || {
        let total_motors = motor_monitor_parameters.number_of_tcp_motor_groups
            + motor_monitor_parameters.number_of_i2c_motor_groups as usize;
        let mut buffers: Vec<MotorGroupSensorsBuffers> = Vec::with_capacity(total_motors);
        for _ in 0..total_motors {
            buffers.push(MotorGroupSensorsBuffers::new(Duration::from_secs_f64(
                motor_monitor_parameters.window_size,
            )))
        }
        let start_time = Duration::from_secs_f64(motor_monitor_parameters.start_time);
        while let Ok(message) = rx.recv() {
            handle_message(&mut buffers, message, &mut cloud_server, start_time);
        }
    })
}

fn handle_message(
    buffers: &mut [MotorGroupSensorsBuffers],
    message: SensorMessage,
    cloud_server: &mut TcpStream,
    start_time: Duration,
) {
    let motor_group_id: u32 = message.sensor_id.shr(u32::BITS / 2);
    let sensor_id = message.sensor_id.bitand(0xFFFF);
    let motor_group_buffers = get_motor_group_buffers(buffers, motor_group_id);
    add_message_to_sensor_buffer(message, sensor_id, motor_group_buffers);
    let now = utils::get_now_duration();
    motor_group_buffers.refresh_caches(now);
    let rule_violated = rules_engine::violated_rule(motor_group_buffers);
    if let Some(rule) = rule_violated {
        eprintln!("Found rule violation {rule} in motor {motor_group_id}");
        let alert = create_alert(motor_group_id, (now - start_time).as_secs_f64(), rule);
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

fn create_alert(motor_group_id: u32, now: f64, rule: MotorFailure) -> Alert {
    Alert {
        time: now,
        motor_id: motor_group_id as u16,
        failure: rule,
    }
}

fn save_benchmark_readings() {
    let me = Process::myself().expect("Could not get process info handle");
    let (cstime, cutime) = me
        .tasks()
        .unwrap()
        .flatten()
        .map(|task| task.stat().unwrap())
        .fold((0, 0), |(stime, utime), task_stat| {
            (stime + task_stat.stime, utime + task_stat.utime)
        });
    let stat = me.stat().expect("Could not get /proc/[pid]/stat info");
    let status = me.status().expect("Could not get /proc/[pid]/status info");
    let benchmark_data = BenchmarkData {
        id: 0,
        time_spent_in_user_mode: stat.utime,
        time_spent_in_kernel_mode: stat.stime,
        children_time_spent_in_user_mode: cutime,
        children_time_spent_in_kernel_mode: cstime,
        peak_resident_set_size: status.vmhwm.expect("Could not get vmhw"),
        peak_virtual_memory_size: status.vmpeak.expect("Could not get vmrss"),
        benchmark_data_type: BenchmarkDataType::MotorMonitor,
    };
    let vec: Vec<u8> =
        to_allocvec_cobs(&benchmark_data).expect("Could not write benchmark data to Vec<u8>");
    let _ = std::io::stdout()
        .write(&vec)
        .expect("Could not write benchmark data bytes to stdout");
    eprintln!("Wrote benchmark data");
}
