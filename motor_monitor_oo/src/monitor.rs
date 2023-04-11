use std::io::Write;
use std::net::TcpStream;
use std::ops::{BitAnd, Shr};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use data_transfer_objects::Alert;
use log::{debug, info};
use postcard::to_allocvec_cobs;

use crate::sensor::SensorAverage;

pub struct MotorMonitor {
    // motor_id: u32,
    pub sensor_data_receiver: Receiver<SensorAverage>,
    pub cloud_server: TcpStream,
    pub air_temperature: Option<SensorAverage>,
    pub process_temperature: Option<SensorAverage>,
    pub rotational_speed: Option<SensorAverage>,
    pub torque: Option<SensorAverage>,
    pub age: Duration,
}

impl MotorMonitor {
    pub fn run(mut self) {
        while let Ok(sensor_average) = self.sensor_data_receiver.recv() {
            let motor_id = sensor_average.sensor_id.shr(2);
            let sensor_id = sensor_average.sensor_id.bitand(0x0003);
            match sensor_id {
                0 => self.air_temperature = Some(sensor_average),
                1 => self.process_temperature = Some(sensor_average),
                2 => self.rotational_speed = Some(sensor_average),
                3 => self.torque = Some(sensor_average),
                _ => panic!("Invalid MotorGroupSensorsBuffers index"),
            };
            if let Some(air_temperature) = &self.air_temperature && let Some(process_temperature) = &self.process_temperature
                && let Some(rotational_speed) = &self.rotational_speed && let Some(torque) = &self.torque
            {
                if let Some(failure) = utils::sensor_data_indicates_failure(air_temperature.average, process_temperature.average, rotational_speed.average, torque.average, utils::get_now_duration().checked_sub(self.age).unwrap_or(Duration::from_secs(0))) {
                    info!("Found rule violation {failure} in motor {}", motor_id);
                    let alert = Alert {
                        time: [air_temperature.timestamp, process_temperature.timestamp, rotational_speed.timestamp, torque.timestamp].into_iter().reduce(f64::max).unwrap(),
                        motor_id: motor_id as u16,
                        failure,
                    };
                    let vec: Vec<u8> =
                        to_allocvec_cobs(&alert).expect("Could not write motor monitor alert to Vec<u8>");
                    self.cloud_server
                        .write_all(&vec)
                        .expect("Could not send motor alert to cloud server");
                    self.process_temperature = None;
                    self.air_temperature = None;
                    self.rotational_speed = None;
                    self.torque = None;
                    self.age = utils::get_now_duration();
                }
            }
        }
        debug!("Exiting monitor");
    }
}