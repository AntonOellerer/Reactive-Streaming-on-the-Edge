use std::io::Write;
use std::net::TcpStream;
use std::ops::{BitAnd, Shr};
use std::sync::mpsc::Receiver;

use log::{debug, info};
use postcard::to_allocvec_cobs;

use data_transfer_objects::Alert;

use crate::sensor::SensorAverage;

pub struct MotorMonitor {
    // motor_id: u32,
    pub sensor_data_receiver: Receiver<SensorAverage>,
    pub cloud_server: TcpStream,
    pub air_temperature: Option<SensorAverage>,
    pub process_temperature: Option<SensorAverage>,
    pub rotational_speed: Option<SensorAverage>,
    pub torque: Option<SensorAverage>,
}

impl MotorMonitor {
    pub fn build(
        sensor_data_receiver: Receiver<SensorAverage>,
        cloud_server: TcpStream,
    ) -> MotorMonitor {
        MotorMonitor {
            sensor_data_receiver,
            cloud_server,
            air_temperature: None,
            process_temperature: None,
            rotational_speed: None,
            torque: None,
        }
    }

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
            if let Some(air_temperature) = &self.air_temperature {
                if let Some(process_temperature) = &self.process_temperature {
                    if let Some(rotational_speed) = &self.rotational_speed {
                        if let Some(torque) = &self.torque {
                            let avg_number_of_values = (air_temperature.number_of_values
                                + process_temperature.number_of_values
                                + rotational_speed.number_of_values
                                + torque.number_of_values)
                                / 4;
                            if let Some(failure) = utils::averages_indicate_failure(
                                air_temperature.average,
                                process_temperature.average,
                                rotational_speed.average,
                                torque.average,
                                avg_number_of_values,
                            ) {
                                info!("Found rule violation {failure} in motor {}", motor_id);
                                let alert = Alert {
                                    time: [
                                        air_temperature.timestamp,
                                        process_temperature.timestamp,
                                        rotational_speed.timestamp,
                                        torque.timestamp,
                                    ]
                                    .into_iter()
                                    .reduce(f64::max)
                                    .unwrap(),
                                    motor_id: motor_id as u16,
                                    failure,
                                };
                                let vec: Vec<u8> = to_allocvec_cobs(&alert)
                                    .expect("Could not write motor monitor alert to Vec<u8>");
                                self.cloud_server
                                    .write_all(&vec)
                                    .expect("Could not send motor alert to cloud server");
                                self.process_temperature = None;
                                self.air_temperature = None;
                                self.rotational_speed = None;
                                self.torque = None;
                            }
                        }
                    }
                }
            }
        }
        debug!("Exiting monitor");
    }
}
