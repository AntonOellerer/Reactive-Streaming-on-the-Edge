use data_transfer_objects::MotorFailure;

use crate::MotorGroupSensorsBuffers;

pub fn violated_rule(motor_group_buffers: &MotorGroupSensorsBuffers) -> Option<MotorFailure> {
    let air_temperature = motor_group_buffers
        .air_temperature_sensor
        .get_window_average();
    let process_temperature = motor_group_buffers
        .process_temperature_sensor
        .get_window_average();
    let rotational_speed = motor_group_buffers
        .rotational_speed_sensor
        .get_window_average();
    let torque = motor_group_buffers.torque_sensor.get_window_average();
    let age = utils::get_now_duration() - motor_group_buffers.age;
    utils::sensor_data_indicates_failure(
        air_temperature,
        process_temperature,
        rotational_speed,
        torque,
        age,
    )
}
