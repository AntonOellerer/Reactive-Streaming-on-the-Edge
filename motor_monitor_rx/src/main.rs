#![feature(drain_filter)]

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::ops::{Add, BitAnd, Index, IndexMut, Shr};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use futures::executor::ThreadPoolBuilder;
use futures::future::RemoteHandle;
use postcard::to_allocvec_cobs;
use procfs::process::Process;
use rx_rust_mp::create::create;
use rx_rust_mp::from_iter::from_iter;
use rx_rust_mp::observable::Observable;
use rx_rust_mp::observer::Observer;

use data_transfer_objects::{
    Alert, BenchmarkData, BenchmarkDataType, MotorFailure, MotorMonitorParameters,
    RequestProcessingModel, SensorMessage,
};

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

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let motor_monitor_parameters: MotorMonitorParameters = get_motor_monitor_parameters(&arguments);
    let sleep_duration = utils::get_duration_to_end(
        Duration::from_secs_f64(motor_monitor_parameters.start_time),
        Duration::from_secs_f64(motor_monitor_parameters.duration),
    )
    .add(Duration::from_secs(1)); //to account for all sensor messages
    let mut cloud_server = TcpStream::connect(format!(
        "localhost:{}",
        motor_monitor_parameters.cloud_server_port
    ))
    .expect("Could not open connection to cloud server");
    let handle = execute_reactive_streaming_procedure(&motor_monitor_parameters, &mut cloud_server);
    eprintln!("Sleeping {sleep_duration:?}");
    thread::sleep(sleep_duration);
    eprintln!("Woke up");
    drop(handle);
    thread::sleep(Duration::from_secs(1));
    save_benchmark_readings();
}

fn execute_reactive_streaming_procedure(
    motor_monitor_parameters: &MotorMonitorParameters,
    cloud_server: &mut TcpStream,
) -> RemoteHandle<()> {
    let mut cloud_server = cloud_server
        .try_clone()
        .expect("Could not clone tcp stream");
    let pool = ThreadPoolBuilder::new()
        .pool_size(motor_monitor_parameters.thread_pool_size)
        .create()
        .unwrap();
    let total_number_of_motors = motor_monitor_parameters.number_of_tcp_motor_groups
        + motor_monitor_parameters.number_of_i2c_motor_groups as usize;
    let motor_ages: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(
        (0..total_number_of_motors)
            .map(|_| utils::get_now_duration())
            .collect(),
    ));
    let start_time = Duration::from_secs_f64(motor_monitor_parameters.start_time);
    let port = motor_monitor_parameters.sensor_port;
    create(
        move |subscriber| match TcpListener::bind(format!("127.0.0.1:{port}")) {
            Ok(listener) => {
                eprintln!("Bound listener on port {port}");
                for stream in listener.incoming() {
                    match stream {
                        Ok(stream) => {
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
                        let mut vec = arc_clone.lock().unwrap();
                        let motor_age = vec[motor_id as usize];
                        // eprintln!("Motor data: {motor_data:?}");
                        violated_rule(&motor_data, motor_age).map(|violated_rule| {
                            let now = utils::get_now_duration();
                            vec[motor_id as usize] = now;
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
    let rotational_speed_in_rad = utils::rpm_to_rad(rotational_speed);
    let age = utils::get_now_duration() - motor_age;
    // eprintln!(
    //     "{} {}",
    //     (air_temperature - process_temperature).abs(),
    //     rotational_speed
    // );
    // eprintln!("{}", torque * rotational_speed_in_rad);
    // eprintln!("{}", age.as_secs_f64() * torque.round());
    eprintln!(
        "temp: {:5.2}, rs: {:5.2}, power: {:5.2}, wear: {:5.2}",
        (air_temperature - process_temperature).abs(),
        rotational_speed,
        torque * rotational_speed_in_rad,
        age.as_secs_f64() * torque.round()
    );
    if (air_temperature - process_temperature).abs() < 8.6 && rotational_speed < 1380.0 {
        Some(MotorFailure::HeatDissipationFailure)
    } else if torque * rotational_speed_in_rad < 3500.0 || torque * rotational_speed_in_rad > 9000.0
    {
        Some(MotorFailure::PowerFailure)
    } else if age.as_secs_f64() * torque > 11_000_f64 {
        Some(MotorFailure::OverstrainFailure)
    } else {
        None
    }
}

fn get_motor_id(sensor_id: u32) -> u32 {
    sensor_id.shr(u32::BITS / 2)
}

fn get_sensor_id(sensor_id: u32) -> u32 {
    sensor_id.bitand(0xFFFF)
}

fn save_benchmark_readings() {
    let me = Process::myself().expect("Could not get process info handle");
    let (stime, utime) = me
        .tasks()
        .unwrap()
        .flatten()
        .map(|task| task.stat().unwrap())
        .fold((0, 0), |(stime, utime), task_stat| {
            (
                stime + task_stat.cutime, //+ task_stat.cstime as u64,
                utime + task_stat.utime,  // + task_stat.cutime as u64,
            )
        });
    let (vmhwm, vmrss) = me
        .tasks()
        .unwrap()
        .flatten()
        .map(|task| task.status().unwrap())
        .fold((0, 0), |(vmhwm, vmrss), task_status| {
            (
                vmhwm + task_status.vmhwm.unwrap_or(0),
                vmrss + task_status.vmrss.unwrap_or(0),
            )
        });
    let stat = me.stat().expect("Could not get /proc/[pid]/stat info");
    let status = me.status().expect("Could not get /proc/[pid]/status info");
    let benchmark_data = BenchmarkData {
        id: 0,
        time_spent_in_user_mode: stat.utime,
        time_spent_in_kernel_mode: utime,
        children_time_spent_in_user_mode: stat.cutime,
        children_time_spent_in_kernel_mode: stime,
        memory_high_water_mark: vmhwm + status.vmhwm.expect("Could not get vmhw"),
        memory_resident_set_size: vmrss + status.vmrss.expect("Could not get vmrss"),
        benchmark_data_type: BenchmarkDataType::MotorMonitor,
    };
    let vec: Vec<u8> =
        to_allocvec_cobs(&benchmark_data).expect("Could not write benchmark data to Vec<u8>");
    let _ = std::io::stdout()
        .write(&vec)
        .expect("Could not write benchmark data bytes to stdout");
    eprintln!("Wrote benchmark data");
}
