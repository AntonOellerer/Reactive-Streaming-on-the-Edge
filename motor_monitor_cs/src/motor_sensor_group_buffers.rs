use std::ops::{Index, IndexMut};

use libc::time_t;

use crate::SlidingWindow;

#[derive(Debug)]
pub struct MotorGroupSensorsBuffers {
    pub air_temperature_sensor: SlidingWindow,
    pub process_temperature_sensor: SlidingWindow,
    pub rotational_speed_sensor: SlidingWindow,
    pub torque_sensor: SlidingWindow,
    pub age: time_t,
}

impl MotorGroupSensorsBuffers {
    pub fn new(window_size: u32) -> MotorGroupSensorsBuffers {
        MotorGroupSensorsBuffers {
            air_temperature_sensor: SlidingWindow::new(window_size),
            process_temperature_sensor: SlidingWindow::new(window_size),
            rotational_speed_sensor: SlidingWindow::new(window_size),
            torque_sensor: SlidingWindow::new(window_size),
            age: utils::get_now(),
        }
    }

    pub fn refresh_caches(&mut self, at_time: time_t) {
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
        self.age = utils::get_now();
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
