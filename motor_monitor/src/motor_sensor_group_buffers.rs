use crate::SlidingWindow;
use libc::time_t;

#[derive(Debug)]
pub struct MotorGroupSensorsBuffers {
    pub air_temperature_sensor: SlidingWindow,
    pub process_temperature_sensor: SlidingWindow,
    pub rotational_speed_sensor: SlidingWindow,
    pub torque_sensor: SlidingWindow,
    pub age: time_t,
}

impl MotorGroupSensorsBuffers {
    pub fn new(window_size: i64) -> MotorGroupSensorsBuffers {
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
