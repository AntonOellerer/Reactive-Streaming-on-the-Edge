#![feature(drain_filter)]

use data_transfer_objects::{
    Alert, BenchmarkDataType, MotorFailure, MotorMonitorParameters, SensorMessage,
};
use env_logger::Target;
use futures::executor::{ThreadPool, ThreadPoolBuilder};
use futures::future::RemoteHandle;
use log::{debug, info, trace};
use postcard::to_allocvec_cobs;
use rx_rust_mp::create::create;
use rx_rust_mp::from_iter::from_iter;
use rx_rust_mp::observable::Observable;
use rx_rust_mp::observer::Observer;
use std::f64;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::ops::{BitAnd, Index, IndexMut, Shr};
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[derive(Debug, Copy, Clone)]
struct SensorAverage {
    reading: f64,
    sensor_id: u32,
    timestamp: f64,
}

#[derive(Debug, Copy, Clone, Default)]
struct MotorData {
    air_temperature_data: Option<SensorAverage>,
    process_temperature_data: Option<SensorAverage>,
    rotational_speed_data: Option<SensorAverage>,
    torque_data: Option<SensorAverage>,
}

impl MotorData {
    fn contains_all_data(&self) -> bool {
        self.air_temperature_data.is_some()
            && self.process_temperature_data.is_some()
            && self.rotational_speed_data.is_some()
            && self.torque_data.is_some()
    }

    fn get_time(&self) -> f64 {
        [
            self.air_temperature_data,
            self.process_temperature_data,
            self.rotational_speed_data,
            self.torque_data,
        ]
        .as_ref()
        .iter()
        .flatten()
        .map(|sensor_message| sensor_message.timestamp)
        .reduce(f64::max)
        .expect("Trying to extract timestamp from empty motor data")
    }
}

impl Index<usize> for MotorData {
    type Output = Option<SensorAverage>;

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

fn main() {
    env_logger::builder().target(Target::Stderr).init();
    let arguments: Vec<String> = std::env::args().collect();
    let motor_monitor_parameters: MotorMonitorParameters =
        utils::get_motor_monitor_parameters(&arguments);
    let mut cloud_server =
        TcpStream::connect(motor_monitor_parameters.motor_monitor_listen_address)
            .expect("Could not open connection to cloud server");
    let pool = ThreadPoolBuilder::new()
        .pool_size(motor_monitor_parameters.thread_pool_size)
        .create()
        .unwrap();
    info!("Running procedure");
    let handle =
        execute_reactive_streaming_procedure(&motor_monitor_parameters, &mut cloud_server, pool);
    futures::executor::block_on(handle);
    info!("Processing completed");
    utils::save_benchmark_readings(0, BenchmarkDataType::MotorMonitor);
    info!("Saved benchmark readings");
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
    let listen_pool = ThreadPoolBuilder::new().pool_size(1).create().unwrap();
    let read_message_pool = ThreadPoolBuilder::new()
        .pool_size(motor_monitor_parameters.number_of_tcp_motor_groups * 4 * 2)
        .create()
        .unwrap();
    let sensor_listen_address = motor_monitor_parameters.sensor_listen_address;
    create(move |subscriber| {
        info!("Listening on {sensor_listen_address}");
        match TcpListener::bind(sensor_listen_address) {
            Ok(listener) => {
                info!("Bound listener on sensor listener address {sensor_listen_address}");
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
        }
        info!("Bound to all sensors");
    })
    .subscribe_on(listen_pool)
    .flat_map(|mut stream| {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("Could not set read timeout");
        create(move |subscriber| {
            while let Some(sensor_message) = utils::read_object::<SensorMessage>(&mut stream) {
                trace!("{sensor_message:?}");
                subscriber.next(sensor_message).unwrap();
            }
            info!("Reading from sensor completed");
        })
    })
    .subscribe_on(read_message_pool)
    .sliding_window(
        Duration::from_millis(motor_monitor_parameters.sampling_interval as u64),
        Duration::from_secs_f64(motor_monitor_parameters.window_size),
        |timed_sensor_message: &SensorMessage| {
            Duration::from_secs_f64(timed_sensor_message.timestamp)
        },
    )
    .flat_map(move |timed_sensor_messages| {
        let arc_clone = Arc::clone(&motor_ages);
        // eprintln!("Messages: {timed_sensor_messages:?}");
        from_iter(timed_sensor_messages)
            .group_by(|message: &SensorMessage| message.sensor_id)
            .flat_map(move |sensor_messages| {
                let sensor_id = sensor_messages.key;
                sensor_messages
                    .map(|message: SensorMessage| (message.reading, message.timestamp))
                    .reduce(
                        (0f64, 0f64, 0f64),
                        |(i, reading, time), (new_reading, new_time)| {
                            (
                                i + 1f64,
                                reading + new_reading as f64,
                                f64::max(time, new_time),
                            )
                        },
                    )
                    .map(move |(i, sum_reading, max_time)| SensorAverage {
                        sensor_id,
                        reading: sum_reading / i,
                        timestamp: max_time,
                    })
            })
            .group_by(|sensor_message| get_motor_id(sensor_message.sensor_id))
            .flat_map(move |motor_group| {
                let motor_id = motor_group.key;
                let arc_clone = Arc::clone(&arc_clone);
                motor_group
                    .reduce(
                        MotorData::default(),
                        move |mut sensor_data, sensor_average| {
                            sensor_data[get_sensor_id(sensor_average.sensor_id) as usize] =
                                Some(sensor_average);
                            sensor_data
                        },
                    )
                    .map(move |motor_data| {
                        let vec = arc_clone.read().unwrap();
                        let motor_age = vec[motor_id as usize];
                        drop(vec);
                        violated_rule(&motor_data, motor_age).map(|violated_rule| {
                            let now = utils::get_now_duration();
                            let mut vec = arc_clone.write().unwrap();
                            vec[motor_id as usize] = now;
                            drop(vec);
                            Alert {
                                time: motor_data.get_time(),
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
            info!("{alert:?}");
            let vec: Vec<u8> =
                to_allocvec_cobs(&alert).expect("Could not write motor monitor alert to Vec<u8>");
            cloud_server
                .write_all(&vec)
                .expect("Could not send motor alert to cloud server");
            debug!("Sent alert to server");
        },
        pool,
    )
}

fn violated_rule(sensor_average_readings: &MotorData, motor_age: Duration) -> Option<MotorFailure> {
    if !sensor_average_readings.contains_all_data() {
        trace!("{sensor_average_readings:?}");
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
    debug!(
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
    sensor_id.shr(2)
}

fn get_sensor_id(sensor_id: u32) -> u32 {
    sensor_id.bitand(0x0003)
}
