use data_transfer_objects::MotorFailure;
use libc::time_t;

use crate::MotorGroupSensorsBuffers;

/**
1. heat dissipation failure (HDF) heat dissipation causes a process failure,
    if the difference between air- and process temperature is below 8.6 K and the tool’s rotational speed is below 1380 rpm
2. power failure (PWF) the product of torque and rotational speed (in rad/s) equals the power
    required for the process. If this power is below 3500 W or above 9000 W, the process fails.
3. overstrain failure (OSF) if the product of tool wear and torque exceeds 11,000 minNm for the L
    product variant (12,000 for M, 13,000 for H), the process fails due to overstrain.
 **/

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
    let rotational_speed_in_rad = utils::rpm_to_rad(rotational_speed);
    let torque = motor_group_buffers.torque_sensor.get_window_average();
    let age = utils::get_now() - motor_group_buffers.age;
    if (air_temperature - process_temperature).abs() < 8.6 && rotational_speed < 1380.0 {
        Some(MotorFailure::HeatDissipationFailure)
    } else if torque * rotational_speed_in_rad < 3500.0 || torque * rotational_speed_in_rad > 9000.0
    {
        Some(MotorFailure::PowerFailure)
    } else if age * torque.round() as time_t > 11_000 {
        Some(MotorFailure::OverstrainFailure)
    } else {
        None
    }
}
