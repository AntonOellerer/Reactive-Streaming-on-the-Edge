#![feature(drain_filter)]

use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::ops::{BitAnd, Shl, Shr};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::executor::{LocalPool, ThreadPool};
use postcard::to_allocvec_cobs;
use rxrust::prelude::SubscribeNext;
use rxrust::prelude::*;

use data_transfer_objects::{
    Alert, MotorFailure, MotorMonitorParameters, RequestProcessingModel, SensorMessage,
};

use crate::rx_utils::HelperTrait;

mod rx_utils;
mod sliding_window;

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
    let mut cloud_server = TcpStream::connect(format!(
        "localhost:{}",
        motor_monitor_parameters.cloud_server_port
    ))
    .expect("Could not open connection to cloud server");
    execute_reactive_streaming_procedure(&motor_monitor_parameters, &mut cloud_server);
}

fn execute_reactive_streaming_procedure(
    motor_monitor_parameters: &MotorMonitorParameters,
    cloud_server: &mut TcpStream,
) {
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
    let port = motor_monitor_parameters.start_port;
    create(move |subscriber| {
        eprintln!("In create function");
        if let Ok(listener) = TcpListener::bind(format!("127.0.0.1:{port}")) {
            eprintln!("Bound listener on port {port}");
            let mut stream = listener.accept().unwrap().0;
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("Could not set read timeout");
            eprintln!("Accepted Stream");
            while let Some(sensor_message) = utils::read_object::<SensorMessage>(&mut stream) {
                eprintln!("Read message {sensor_message:?}");
                subscriber.next(sensor_message)
            }
            // for interval in 0..3 {
            //     // let sensor_message = utils::read_object::<SensorMessage>(&mut stream).unwrap();
            //     eprintln!("Read message");
            //     subscriber.next(SensorMessage {
            //         reading: 0.0,
            //         sensor_id: interval.shl(16) as u32,
            //     });
            // }
            //     drop(listener);
        } else {
            // subscriber.complete();
            return;
        }
        eprintln!("Completing");
        subscriber.complete();
    })
    .map(TimedSensorMessage::from)
    .group_by(|sensor_message: &TimedSensorMessage| {
        eprintln!("Grouping");
        sensor_message.sensor_id
    })
    .flat_map(move |sensor_group| {
        let sensor_id = sensor_group.key;
        sensor_group
            .sliding_window(
                Duration::from_secs(1),
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
    // .flat_map(|motor_group| {
    //     eprintln!("Hello from the flat map");
    //     motor_group
    //         .group_by(|sensor_message: &TimedSensorMessage| sensor_message.sensor_id)
    //         .map(|sensor_group| {
    //             let mut average: Option<f64> = None;
    //             sensor_group
    //                 .map(|sensor_message: TimedSensorMessage| {
    //                     sensor_message.reading as f64
    //                 })
    //                 .average()
    //                 .subscribe(|value| average = Some(value));
    //             (sensor_group.key, average)
    //         })
    //         .filter_map(|(sensor_id, average): (Option<u32>, Option<f64>)| {
    //             match (sensor_id, average) {
    //                 (Some(sensor_id), Some(average)) => Some((sensor_id, average)),
    //                 _ => None,
    //             }
    //         })
    //         .group_by(|(sensor_id, _): &(u32, f64)| get_motor_id(*sensor_id))
    //         .flat_map(|group| {
    //             group
    //                 //todo write object for hashmap like the buffers?
    //                 .reduce_initial(
    //                     (0usize, HashMap::new()),
    //                     |(_, mut map), (motor_sensor_id, average)| {
    //                         map.insert(get_sensor_id(motor_sensor_id), average);
    //                         (get_motor_id(motor_sensor_id) as usize, map)
    //                     },
    //                 )
    //                 .filter_map(|(motor_id, value_map)| {
    //                     let arc = Arc::clone(&motor_ages);
    //                     let mut vec = (*arc).lock().unwrap();
    //                     let motor_age = vec[motor_id];
    //                     if let Some(failure) = violated_rule(&value_map, motor_age) {
    //                         let now = utils::get_now_duration();
    //                         vec[motor_id] = now;
    //                         Some(Alert {
    //                             time: now.as_secs_f64(),
    //                             motor_id: motor_id as u16,
    //                             failure,
    //                         })
    //                     } else {
    //                         None
    //                     }
    //                 })
    //         })
    // })
    // .subscribe(|alert| {
    //     eprintln!("In subscribe");
    //     let vec: Vec<u8> =
    //         to_allocvec_cobs(&alert).expect("Could not write motor monitor alert to Vec<u8>");
    //     cloud_server
    //         .write_all(&vec)
    //         .expect("Could not send motor alert to cloud server");
    // });
    // .subscribe_on(pool.spawner())
    // .into_shared()
    .subscribe(|group| {
        eprintln!("In subscribe");
        // group.subscribe(|item| eprintln!("{:?}", item));
        eprintln!("{group:?}");
    });
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

fn get_sensor_id(sensor_id: u32) -> u32 {
    sensor_id.bitand(0xFFFF)
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::sync::Arc;

    use futures::executor::LocalPool;
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
        //the problem might be, that each new subscription, which is done per group again, subscribes to a new instance of the subscriber -> the listener
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
}
