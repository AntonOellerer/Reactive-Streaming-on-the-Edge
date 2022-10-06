use std::fmt::{Display, Formatter};
use std::str::FromStr;

use libc::time_t;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Copy, Clone)]
pub enum RequestProcessingModel {
    ReactiveStreaming,
    ClientServer,
}

impl FromStr for RequestProcessingModel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ReactiveStreaming" => Ok(RequestProcessingModel::ReactiveStreaming),
            "ClientServer" => Ok(RequestProcessingModel::ClientServer),
            _ => Err(()),
        }
    }
}

impl ToString for RequestProcessingModel {
    fn to_string(&self) -> String {
        match self {
            RequestProcessingModel::ReactiveStreaming => "ReactiveStreaming",
            RequestProcessingModel::ClientServer => "ClientServer",
        }
        .to_string()
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MotorFailure {
    ToolWearFailure,
    HeatDissipationFailure,
    PowerFailure,
    OverstrainFailure,
    RandomFailure,
}

impl Display for MotorFailure {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SensorParameters {
    pub id: u32,
    pub start_time: time_t,
    pub duration: u64,
    pub seed: u32,
    pub sampling_interval: u32,
    pub request_processing_model: RequestProcessingModel,
    pub motor_monitor_port: u16,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SensorBenchmarkData {
    pub id: u32,
    pub time_spent_in_user_mode: u64,
    pub time_spent_in_kernel_mode: u64,
    pub children_time_spent_in_user_mode: i64,
    pub children_time_spent_in_kernel_mode: i64,
    pub memory_high_water_mark: u64,
    pub memory_resident_set_size: u64,
}

impl SensorBenchmarkData {
    pub fn to_csv_string(&self) -> String {
        format!(
            "{},{},{},{},{},{},{}\n",
            self.id,
            self.time_spent_in_user_mode,
            self.time_spent_in_kernel_mode,
            self.children_time_spent_in_user_mode,
            self.children_time_spent_in_kernel_mode,
            self.memory_high_water_mark,
            self.memory_resident_set_size
        )
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MotorMonitorBenchmarkData {
    pub time_spent_in_user_mode: u64,
    pub time_spent_in_kernel_mode: u64,
    pub children_time_spent_in_user_mode: i64,
    pub children_time_spent_in_kernel_mode: i64,
    pub memory_high_water_mark: u64,
    pub memory_resident_set_size: u64,
}

impl MotorMonitorBenchmarkData {
    pub fn to_csv_string(&self) -> String {
        format!(
            "{},{},{},{},{},{}\n",
            self.time_spent_in_user_mode,
            self.time_spent_in_kernel_mode,
            self.children_time_spent_in_user_mode,
            self.children_time_spent_in_kernel_mode,
            self.memory_high_water_mark,
            self.memory_resident_set_size
        )
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SensorMessage {
    pub timestamp: time_t,
    pub reading: f32,
    pub sensor_id: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MotorMonitorParameters {
    pub start_time: time_t,
    pub duration: u64,
    pub request_processing_model: RequestProcessingModel,
    pub number_of_motor_groups: usize,
    pub window_size: i64,
    pub start_port: u16,
    pub cloud_server_port: u16,
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct MotorDriverRunParameters {
    pub start_time: time_t,
    pub duration: u64,
    pub number_of_motor_groups: usize,
    pub window_size: i64,
    pub sensor_start_port: u16,
    pub sampling_interval: u32,
    pub request_processing_model: RequestProcessingModel,
    pub cloud_server_port: u16,
    pub sensor_driver_start_port: u16,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Alert {
    pub time: time_t,
    pub motor_id: u16,
    pub failure: MotorFailure,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CloudServerRunParameters {
    pub start_time: time_t,
    pub duration: u64,
    pub motor_monitor_port: u16,
    pub request_processing_model: RequestProcessingModel,
}
