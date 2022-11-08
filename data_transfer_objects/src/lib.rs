#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(feature = "std")]
use std::fmt;
#[cfg(feature = "std")]
use std::fmt::Formatter;
#[cfg(feature = "std")]
use std::str::FromStr;

#[cfg(feature = "std")]
use libc::time_t;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Copy, Clone)]
pub enum RequestProcessingModel {
    ReactiveStreaming,
    ClientServer,
}

#[cfg(feature = "std")]
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

#[cfg(feature = "std")]
impl ToString for RequestProcessingModel {
    fn to_string(&self) -> String {
        match self {
            RequestProcessingModel::ReactiveStreaming => "ReactiveStreaming",
            RequestProcessingModel::ClientServer => "ClientServer",
        }
        .to_string()
    }
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum MotorFailure {
    ToolWearFailure,
    HeatDissipationFailure,
    PowerFailure,
    OverstrainFailure,
    RandomFailure,
}

#[cfg(feature = "std")]
impl fmt::Display for MotorFailure {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[cfg(feature = "std")]
impl FromStr for MotorFailure {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ToolWearFailure" => Ok(MotorFailure::ToolWearFailure),
            "HeatDissipationFailure" => Ok(MotorFailure::HeatDissipationFailure),
            "PowerFailure" => Ok(MotorFailure::PowerFailure),
            "OverstrainFailure" => Ok(MotorFailure::OverstrainFailure),
            "RandomFailure" => Ok(MotorFailure::RandomFailure),
            _ => Err(()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SensorParameters {
    pub id: u32,
    pub duration: u32,
    pub sampling_interval: u32,
    pub request_processing_model: RequestProcessingModel,
    pub motor_monitor_port: u16,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BenchmarkData {
    pub id: u32,
    pub time_spent_in_user_mode: u64,
    pub time_spent_in_kernel_mode: u64,
    pub children_time_spent_in_user_mode: i64,
    pub children_time_spent_in_kernel_mode: i64,
    pub memory_high_water_mark: u64,
    pub memory_resident_set_size: u64,
    pub benchmark_data_type: BenchmarkDataType,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum BenchmarkDataType {
    Sensor,
    MotorMonitor,
}

#[cfg(feature = "std")]
impl BenchmarkData {
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

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct SensorMessage {
    pub reading: f32,
    pub sensor_id: u32,
}

#[cfg(feature = "std")]
#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct MotorMonitorParameters {
    pub start_time: time_t,
    pub duration: u32,
    pub request_processing_model: RequestProcessingModel,
    pub number_of_tcp_motor_groups: usize,
    pub number_of_i2c_motor_groups: u8,
    pub window_size: u32,
    pub sensor_port: u16,
    pub cloud_server_port: u16,
}

#[cfg(feature = "std")]
#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct MotorDriverRunParameters {
    pub start_time: time_t,
    pub duration: u32,
    pub number_of_tcp_motor_groups: usize,
    pub number_of_i2c_motor_groups: u8,
    pub window_size_seconds: u32,
    pub sensor_port: u16,
    pub sampling_interval: u32,
    pub request_processing_model: RequestProcessingModel,
    pub cloud_server_port: u16,
    pub sensor_driver_start_port: u16,
}

#[cfg(feature = "std")]
#[derive(Serialize, Deserialize, Debug)]
pub struct Alert {
    pub time: time_t,
    pub motor_id: u16,
    pub failure: MotorFailure,
}

#[cfg(feature = "std")]
impl Alert {
    pub fn to_csv(&self) -> String {
        format!("{},{},{}\n", self.motor_id, self.time, self.failure)
    }

    pub fn from_csv(csv_line: String) -> Alert {
        let values: Vec<&str> = csv_line.split(',').collect();
        Alert {
            motor_id: u16::from_str(values[0]).expect("Could not parse motor id"),
            time: time_t::from_str(values[1]).expect("Could not parse time"),
            failure: MotorFailure::from_str(values[2]).expect("Could not parse MotorFailure"),
        }
    }
}

#[cfg(feature = "std")]
#[derive(Serialize, Deserialize, Debug)]
pub struct CloudServerRunParameters {
    pub start_time: time_t,
    pub duration: u32,
    pub motor_monitor_port: u16,
    pub request_processing_model: RequestProcessingModel,
}
