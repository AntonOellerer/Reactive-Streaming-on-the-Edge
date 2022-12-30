#![feature(drain_filter)]

use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::ops::{Add, Shr};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use futures::executor::LocalPool;
use postcard::to_allocvec_cobs;
use procfs::process::Process;
use rxrust::prelude::*;

use data_transfer_objects::{
    Alert, BenchmarkData, BenchmarkDataType, MotorFailure, MotorMonitorParameters,
    RequestProcessingModel, SensorMessage,
};

#[derive(Debug, Copy, Clone)]
pub struct TimedSensorMessage {
    pub timestamp: f64,
    reading: f32,
    sensor_id: u32,
}

impl From<SensorMessage> for TimedSensorMessage {
    fn from(sensor_message: SensorMessage) -> Self {
        TimedSensorMessage {
            timestamp: utils::get_now_secs(),
            reading: sensor_message.reading,
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
    let processing_thread = thread::spawn(move || {
        execute_reactive_streaming_procedure(&motor_monitor_parameters, &mut cloud_server)
    });
    eprintln!("Sleeping {:?}", sleep_duration);
    thread::sleep(sleep_duration);
    eprintln!("Woke up");
    save_benchmark_readings();
    drop(processing_thread);
}

fn execute_reactive_streaming_procedure(
    motor_monitor_parameters: &MotorMonitorParameters,
    cloud_server: &mut TcpStream,
) {
    let mut cloud_server = cloud_server
        .try_clone()
        .expect("Could not clone tcp stream");
    let pool = LocalPool::new();
    let spawner_0 = pool.spawner();
    let spawner_1 = pool.spawner();
    let total_number_of_motors = motor_monitor_parameters.number_of_tcp_motor_groups
        + motor_monitor_parameters.number_of_i2c_motor_groups as usize;
    let motor_ages: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(
        (0..total_number_of_motors)
            .map(|_| utils::get_now_duration())
            .collect(),
    ));
    let port = motor_monitor_parameters.sensor_port;
    create(move |subscriber| {
        match TcpListener::bind(format!("127.0.0.1:{port}")) {
            Ok(listener) => {
                eprintln!("Bound listener on port {port}");
                for stream in listener.incoming() {
                    match stream {
                        Ok(mut stream) => {
                            stream
                                .set_read_timeout(Some(Duration::from_secs(2)))
                                .expect("Could not set read timeout");
                            while let Some(sensor_message) =
                                utils::read_object::<SensorMessage>(&mut stream)
                            {
                                eprintln!("Read message {sensor_message:?}");
                                subscriber.next(sensor_message);
                            }
                        }
                        Err(e) => subscriber.error(e.to_string()),
                    }
                }
            }
            Err(e) => subscriber.error(e.to_string()),
        }
        subscriber.complete();
    })
    .map(TimedSensorMessage::from)
    .group_by(|sensor_message: &TimedSensorMessage| sensor_message.sensor_id)
    .flat_map(move |sensor_group| {
        let sensor_id = sensor_group.key;
        sensor_group
            .sliding_window(
                Duration::from_secs(1),
                Duration::from_millis(250),
                |timed_sensor_message: TimedSensorMessage| {
                    Duration::from_secs_f64(timed_sensor_message.timestamp)
                },
                spawner_0.clone(),
            )
            .filter(|sliding_window| !sliding_window.is_empty())
            .map(move |sliding_window| {
                (
                    sensor_id,
                    sliding_window
                        .iter()
                        .map(|sm: &TimedSensorMessage| sm.reading)
                        .sum::<f32>() as f64
                        / sliding_window.len() as f64,
                    sliding_window.last().unwrap().timestamp,
                )
            })
    })
    .group_by(|sensor_triple| get_motor_id(sensor_triple.0))
    .flat_map(move |motor_group| {
        let motor_id = motor_group.key;
        let arc_clone = Arc::clone(&motor_ages);
        motor_group
            .sliding_window(
                Duration::from_secs(1),
                Duration::from_millis(250),
                |sensor_triple: (u32, f64, f64)| Duration::from_secs_f64(sensor_triple.2),
                spawner_1.clone(),
            )
            .filter(|sliding_window| !sliding_window.is_empty())
            .filter_map(move |sliding_window: Vec<(u32, f64, f64)>| {
                let map = sliding_window
                    .iter()
                    .fold(HashMap::new(), |mut hash_map, triple| {
                        hash_map.insert(triple.0, triple.2);
                        hash_map
                    });
                let arc = Arc::clone(&arc_clone);
                let mut vec = (*arc).lock().unwrap();
                let motor_age = vec[motor_id as usize];
                violated_rule(&map, motor_age).map(|violated_rule| {
                    let now = utils::get_now_duration();
                    vec[motor_id as usize] = now;
                    Alert {
                        time: sliding_window.last().unwrap().2,
                        motor_id: motor_id as u16,
                        failure: violated_rule,
                    }
                })
            })
    })
    .subscribe_err(
        move |alert| {
            eprintln!("{alert:?}");
            let vec: Vec<u8> =
                to_allocvec_cobs(&alert).expect("Could not write motor monitor alert to Vec<u8>");
            cloud_server
                .write_all(&vec)
                .expect("Could not send motor alert to cloud server");
        },
        |e| eprintln!("{e:?}"),
    );
}

fn violated_rule(value_map: &HashMap<u32, f64>, motor_age: Duration) -> Option<MotorFailure> {
    let air_temperature = *value_map
        .get(&0)
        .expect("No air temperature (0) in value map");
    let process_temperature = *value_map
        .get(&1)
        .expect("No process temperature (1) in value map");
    let rotational_speed = *value_map
        .get(&2)
        .expect("No rotational_speed (2) in value map");
    let torque = *value_map.get(&3).expect("No torque (3) in value map");
    let rotational_speed_in_rad = utils::rpm_to_rad(rotational_speed);
    let age = utils::get_now_duration() - motor_age;
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
    eprintln!("Wrote benchmark data");
}

#[cfg(test)]
mod tests {
    use std::io::{Error, ErrorKind};
    use std::net::TcpListener;

    use rxrust::prelude::*;

    use data_transfer_objects::SensorMessage;

    use crate::TimedSensorMessage;

    #[test]
    fn it_can_group() {
        observable::from_iter([
            SensorMessage {
                reading: 0.0,
                sensor_id: 0,
            },
            SensorMessage {
                reading: 4.0,
                sensor_id: 1,
            },
            SensorMessage {
                reading: 6.0,
                sensor_id: 1,
            },
            SensorMessage {
                reading: 2.0,
                sensor_id: 0,
            },
        ])
        .map(TimedSensorMessage::from)
        .group_by(|sensor_message: &TimedSensorMessage| sensor_message.sensor_id)
        .subscribe(|group| {
            group
                .reduce(|acc, sensor_message| format!("{} {}", acc, sensor_message.reading))
                .subscribe(|result| println!("{}", result));
        });
    }

    #[test]
    fn it_groups_with_listener() {
        let obs_count = MutRc::own(0);
        observable::create(|subscriber| {
            println!("trying to subscribe");
            if let Ok(_) = TcpListener::bind("127.0.0.1:8080") {
                subscriber.next(1);
                subscriber.complete();
            };
        })
        .buffer_with_count(1)
        .group_by(|value: &Vec<i64>| value[0])
        .subscribe(|group| {
            let obs_clone = obs_count.clone();
            group.subscribe(move |_| {
                *obs_clone.rc_deref_mut() += 1;
            });
        });
        assert_eq!(1, *obs_count.rc_deref());
    }

    #[test]
    fn it_forwards_errors() {
        let obs_count = MutRc::own(0);
        observable::create(|subscriber| {
            subscriber.next(1);
            subscriber.error(Error::from(ErrorKind::InvalidInput).to_string());
            subscriber.complete();
        })
        .group_by(|value| *value)
        .subscribe_err(
            |group| {
                let obs_clone = obs_count.clone();
                group.subscribe_err(
                    move |_| {
                        *obs_clone.rc_deref_mut() += 1;
                    },
                    |err| eprintln!("{err:?}"),
                );
            },
            |err| eprintln!("{err:?}"),
        );
        assert_eq!(1, *obs_count.rc_deref());
    }
}
