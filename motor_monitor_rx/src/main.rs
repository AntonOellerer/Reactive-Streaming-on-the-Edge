#![feature(drain_filter)]

use std::net::TcpListener;
use std::str::FromStr;

use libc::time_t;
use rxrust::observable;

use data_transfer_objects::{MotorMonitorParameters, RequestProcessingModel, SensorMessage};

mod rx_utils;

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
        start_port: arguments
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
    execute_reactive_streaming_procedure(&motor_monitor_parameters);
}

fn execute_reactive_streaming_procedure(motor_monitor_parameters: &MotorMonitorParameters) {
    observable::create(|subscriber| {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", "8080"))
            .unwrap_or_else(|_| panic!("Could not bind sensor data listener to {}", "8080"));
        let mut stream = listener.accept().unwrap().0;
        loop {
            match utils::read_object::<SensorMessage>(&mut stream) {
                None => subscriber.complete(),
                Some(sensor_message) => subscriber.next(TimedSensorMessage::from(sensor_message)),
            }
        }
        subscriber.error("Not reachable");
    });
    // .sliding_window(
    //     Duration::from_millis(motor_monitor_parameters.window_size as u64),
    //     |sensor_message: TimedSensorMessage| sensor_message.timestamp,
    // );
}
