use std::f64;
use std::ops::{Index, IndexMut};
use std::time::Duration;

use crate::SlidingWindow;

#[derive(Debug)]
pub struct MotorGroupSensorsBuffers {
    pub air_temperature_sensor: SlidingWindow,
    pub process_temperature_sensor: SlidingWindow,
    pub rotational_speed_sensor: SlidingWindow,
    pub torque_sensor: SlidingWindow,
    pub age: Duration,
}

impl MotorGroupSensorsBuffers {
    pub(crate) fn is_some(&self) -> bool {
        self.air_temperature_sensor.len() > 0
            && self.process_temperature_sensor.len() > 0
            && self.rotational_speed_sensor.len() > 0
            && self.torque_sensor.len() > 0
    }
}

impl MotorGroupSensorsBuffers {
    pub fn new(window_size: Duration) -> MotorGroupSensorsBuffers {
        MotorGroupSensorsBuffers {
            air_temperature_sensor: SlidingWindow::new(window_size),
            process_temperature_sensor: SlidingWindow::new(window_size),
            rotational_speed_sensor: SlidingWindow::new(window_size),
            torque_sensor: SlidingWindow::new(window_size),
            age: utils::get_now_duration(),
        }
    }

    pub fn refresh_caches(&mut self, at_time: Duration) {
        self.air_temperature_sensor.refresh_cache(at_time);
        self.process_temperature_sensor.refresh_cache(at_time);
        self.rotational_speed_sensor.refresh_cache(at_time);
        self.torque_sensor.refresh_cache(at_time);
    }

    pub fn reset(&mut self) {
        self.air_temperature_sensor.reset();
        self.process_temperature_sensor.reset();
        self.rotational_speed_sensor.reset();
        self.torque_sensor.reset();
        self.age = utils::get_now_duration();
    }

    pub(crate) fn get_time(&self) -> f64 {
        self.rotational_speed_sensor
            .iter()
            .chain(self.process_temperature_sensor.iter())
            .chain(self.rotational_speed_sensor.iter())
            .chain(self.torque_sensor.iter())
            .map(|sensor_message| sensor_message.timestamp)
            .reduce(f64::max)
            .expect("Trying to get time from empty sensor group buffers")
    }
}

impl Index<usize> for MotorGroupSensorsBuffers {
    type Output = SlidingWindow;

    fn index(&self, index: usize) -> &Self::Output {
        match index {
            0 => &self.air_temperature_sensor,
            1 => &self.process_temperature_sensor,
            2 => &self.rotational_speed_sensor,
            3 => &self.torque_sensor,
            _ => panic!("Invalid MotorGroupSensorsBuffers index"),
        }
    }
}

impl IndexMut<usize> for MotorGroupSensorsBuffers {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match index {
            0 => &mut self.air_temperature_sensor,
            1 => &mut self.process_temperature_sensor,
            2 => &mut self.rotational_speed_sensor,
            3 => &mut self.torque_sensor,
            _ => panic!("Invalid MotorGroupSensorsBuffers index"),
        }
    }
}
