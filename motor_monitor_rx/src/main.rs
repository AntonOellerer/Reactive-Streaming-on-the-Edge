#![feature(drain_filter)]

use data_transfer_objects::{
    Alert, BenchmarkDataType, MotorFailure, MotorMonitorParameters, SensorMessage,
};
use futures::executor::{ThreadPool, ThreadPoolBuilder};
use futures::future::RemoteHandle;
use postcard::to_allocvec_cobs;
use procfs::process::Process;
use rx_rust_mp::create::create;
use rx_rust_mp::from_iter::from_iter;
use rx_rust_mp::observable::Observable;
use rx_rust_mp::observer::Observer;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::ops::{BitAnd, Index, IndexMut, Shr};
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[derive(Debug, Copy, Clone)]
pub struct TimedSensorMessage {
    pub timestamp: f64,
    reading: f64,
    sensor_id: u32,
}

#[derive(Debug, Copy, Clone, Default)]
struct MotorData {
    air_temperature_data: Option<TimedSensorMessage>,
    process_temperature_data: Option<TimedSensorMessage>,
    rotational_speed_data: Option<TimedSensorMessage>,
    torque_data: Option<TimedSensorMessage>,
}

impl MotorData {
    fn contains_all_data(&self) -> bool {
        self.air_temperature_data.is_some()
            && self.process_temperature_data.is_some()
            && self.rotational_speed_data.is_some()
            && self.torque_data.is_some()
    }
}

impl Index<usize> for MotorData {
    type Output = Option<TimedSensorMessage>;

    fn index(&self, index: usize) -> &Self::Output {
        match index {
            0 => &self.air_temperature_data,
            1 => &self.process_temperature_data,
            2 => &self.rotational_speed_data,
            3 => &self.torque_data,
            _ => panic!("Invalid MotorData index"),
        }
    }
}

impl IndexMut<usize> for MotorData {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match index {
            0 => &mut self.air_temperature_data,
            1 => &mut self.process_temperature_data,
            2 => &mut self.rotational_speed_data,
            3 => &mut self.torque_data,
            _ => panic!("Invalid MotorData index"),
        }
    }
}

impl From<SensorMessage> for TimedSensorMessage {
    fn from(sensor_message: SensorMessage) -> Self {
        TimedSensorMessage {
            timestamp: utils::get_now_secs(),
            reading: sensor_message.reading as f64,
            sensor_id: sensor_message.sensor_id,
        }
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let motor_monitor_parameters: MotorMonitorParameters =
        utils::get_motor_monitor_parameters(&arguments);
    let mut cloud_server =
        TcpStream::connect(motor_monitor_parameters.motor_monitor_listen_address)
            .expect("Could not open connection to cloud server");
    eprintln!("process id:{:?}", std::process::id());
    let pool = ThreadPoolBuilder::new()
        .pool_size(motor_monitor_parameters.thread_pool_size)
        .create()
        .unwrap();
    let handle =
        execute_reactive_streaming_procedure(&motor_monitor_parameters, &mut cloud_server, pool);
    eprintln!("Time: {}", Process::myself().unwrap().stat().unwrap().utime);
    futures::executor::block_on(handle);
    eprintln!("Time: {}", Process::myself().unwrap().stat().unwrap().utime);
    utils::save_benchmark_readings(0, BenchmarkDataType::MotorMonitor);
}

fn execute_reactive_streaming_procedure(
    motor_monitor_parameters: &MotorMonitorParameters,
    cloud_server: &mut TcpStream,
    pool: ThreadPool,
) -> RemoteHandle<()> {
    let mut cloud_server = cloud_server
        .try_clone()
        .expect("Could not clone tcp stream");
    let total_number_of_motors = motor_monitor_parameters.number_of_tcp_motor_groups
        + motor_monitor_parameters.number_of_i2c_motor_groups as usize;
    let total_number_of_sensors = total_number_of_motors * 4;
    //todo find a way to replace this
    //maybe replace with rule not depending on age
    let motor_ages: Arc<RwLock<Vec<Duration>>> = Arc::new(RwLock::new(
        (0..total_number_of_motors)
            .map(|_| utils::get_now_duration())
            .collect(),
    ));
    let start_time = Duration::from_secs_f64(motor_monitor_parameters.start_time);
    let sensor_listen_address = motor_monitor_parameters.sensor_listen_address;
    create(
        move |subscriber| match TcpListener::bind(sensor_listen_address) {
            Ok(listener) => {
                eprintln!("Bound listener on sensor listener address {sensor_listen_address}");
                for _ in 0..total_number_of_sensors {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            subscriber.next(stream).unwrap();
                        }
                        Err(e) => subscriber.error(e).unwrap(),
                    }
                }
            }
            Err(e) => subscriber.error(e).unwrap(),
        },
    )
    .flat_map(|mut stream| {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("Could not set read timeout");
        create(move |subscriber| {
            while let Some(sensor_message) = utils::read_object::<SensorMessage>(&mut stream) {
                // eprintln!(
                //     "Read message {sensor_message:?} at {:#?}",
                //     utils::get_now_duration()
                // );
                subscriber.next(sensor_message).unwrap();
            }
        })
    })
    .map(TimedSensorMessage::from)
    .sliding_window(
        Duration::from_millis(motor_monitor_parameters.sampling_interval as u64),
        Duration::from_secs_f64(motor_monitor_parameters.window_size),
        |timed_sensor_message: &TimedSensorMessage| {
            Duration::from_secs_f64(timed_sensor_message.timestamp)
        },
    )
    .flat_map(move |timed_sensor_messages| {
        let arc_clone = Arc::clone(&motor_ages);
        // eprintln!("Messages: {timed_sensor_messages:?}");
        from_iter(timed_sensor_messages)
            .group_by(|message: &TimedSensorMessage| message.sensor_id)
            .flat_map(move |sensor_messages| {
                let sensor_id = sensor_messages.key;
                sensor_messages
                    .map(|message: TimedSensorMessage| message.reading)
                    .average()
                    .map(move |sensor_average| {
                        let last_timestamp = utils::get_now_secs();
                        // eprintln!("{sensor_id}: {sensor_average}");
                        TimedSensorMessage {
                            sensor_id,
                            reading: sensor_average,
                            timestamp: last_timestamp,
                        }
                    })
            })
            .group_by(|sensor_message| get_motor_id(sensor_message.sensor_id))
            .flat_map(move |motor_group| {
                let motor_id = motor_group.key;
                let arc_clone = Arc::clone(&arc_clone);
                motor_group
                    .reduce(
                        MotorData::default(),
                        move |mut sensor_data, sensor_message: TimedSensorMessage| {
                            sensor_data[get_sensor_id(sensor_message.sensor_id) as usize] =
                                Some(sensor_message);
                            sensor_data
                        },
                    )
                    .map(move |motor_data| {
                        // if motor_data.contains_all_data() {
                        //     eprintln!(
                        //         "{motor_id}: at: {:3.2}, pt: {:3.2}, rs: {:4.2}, t: {:2.2}",
                        //         motor_data.air_temperature_data.unwrap().reading,
                        //         motor_data.process_temperature_data.unwrap().reading,
                        //         motor_data.rotational_speed_data.unwrap().reading,
                        //         motor_data.torque_data.unwrap().reading
                        //     );
                        // }
                        let vec = arc_clone.read().unwrap();
                        let motor_age = vec[motor_id as usize];
                        // eprintln!("Motor data: {motor_data:?}");
                        drop(vec);
                        violated_rule(&motor_data, motor_age).map(|violated_rule| {
                            let now = utils::get_now_duration();
                            let mut vec = arc_clone.write().unwrap();
                            vec[motor_id as usize] = now;
                            drop(vec);
                            Alert {
                                time: (utils::get_now_duration() - start_time).as_secs_f64(),
                                motor_id: motor_id as u16,
                                failure: violated_rule,
                            }
                        })
                    })
            })
    })
    .filter(|alert| alert.is_some())
    .map(|alert| alert.unwrap())
    .subscribe(
        move |alert| {
            eprintln!("{alert:?}");
            let vec: Vec<u8> =
                to_allocvec_cobs(&alert).expect("Could not write motor monitor alert to Vec<u8>");
            cloud_server
                .write_all(&vec)
                .expect("Could not send motor alert to cloud server");
        },
        pool,
    )
}

fn violated_rule(sensor_average_readings: &MotorData, motor_age: Duration) -> Option<MotorFailure> {
    if !sensor_average_readings.contains_all_data() {
        return None;
    }
    let air_temperature = sensor_average_readings
        .air_temperature_data
        .unwrap()
        .reading;
    let process_temperature = sensor_average_readings
        .process_temperature_data
        .unwrap()
        .reading;
    let rotational_speed = sensor_average_readings
        .rotational_speed_data
        .unwrap()
        .reading;
    let torque = sensor_average_readings.torque_data.unwrap().reading;
    let age = utils::get_now_duration() - motor_age;
    eprintln!(
        "temp: {:5.2}, rs: {:5.2}, torque: {:5.2}, wear: {:5.2}",
        (air_temperature - process_temperature).abs(),
        rotational_speed,
        torque,
        age.as_secs_f64() * torque.round()
    );
    utils::rule_violated(
        air_temperature,
        process_temperature,
        rotational_speed,
        torque,
        age,
    )
}

fn get_motor_id(sensor_id: u32) -> u32 {
    sensor_id.shr(u32::BITS / 2)
}

fn get_sensor_id(sensor_id: u32) -> u32 {
    sensor_id.bitand(0xFFFF)
}
