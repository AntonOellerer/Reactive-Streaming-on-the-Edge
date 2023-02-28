#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(feature = "std")]
use std::fmt;
#[cfg(feature = "std")]
use std::fmt::Formatter;
use std::net::IpAddr;
#[cfg(feature = "std")]
use std::net::SocketAddr;
use std::ops::Index;
#[cfg(feature = "std")]
use std::str::FromStr;
#[cfg(feature = "std")]
use std::{f32, f64};

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

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Copy, Clone)]
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
    pub start_time: f64,
    pub duration: f64,
    pub sampling_interval: u32,
    pub request_processing_model: RequestProcessingModel,
    pub motor_monitor_listen_address: SocketAddr,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BenchmarkData {
    pub id: u32,
    pub time_spent_in_user_mode: u64,
    pub time_spent_in_kernel_mode: u64,
    pub children_time_spent_in_user_mode: u64,
    pub children_time_spent_in_kernel_mode: u64,
    pub peak_resident_set_size: u64,
    pub peak_virtual_memory_size: u64,
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
            self.peak_resident_set_size,
            self.peak_virtual_memory_size
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct SensorMessage {
    pub reading: f32,
    pub sensor_id: u32,
    pub timestamp: f64,
}

#[cfg(feature = "std")]
#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct MotorMonitorParameters {
    pub start_time: f64,
    pub duration: f64,
    pub request_processing_model: RequestProcessingModel,
    pub number_of_tcp_motor_groups: usize,
    pub number_of_i2c_motor_groups: u8,
    pub window_size_ms: u64,
    pub sensor_listen_address: SocketAddr,
    pub motor_monitor_listen_address: SocketAddr,
    pub sensor_sampling_interval: u32,
    pub window_sampling_interval: u32,
    pub thread_pool_size: usize,
}

#[cfg(feature = "std")]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MotorDriverRunParameters {
    pub start_time: f64,
    pub duration: f64,
    pub number_of_tcp_motor_groups: usize,
    pub number_of_i2c_motor_groups: u8,
    pub window_size_ms: u64,
    pub sensor_listen_address: SocketAddr,
    pub sensor_sampling_interval: u32,
    pub window_sampling_interval: u32,
    pub request_processing_model: RequestProcessingModel,
    pub motor_monitor_listen_address: SocketAddr,
    pub sensor_socket_addresses: Vec<SocketAddr>,
    pub thread_pool_size: usize,
}

#[cfg(feature = "std")]
#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct Alert {
    pub time: f64,
    pub motor_id: u16,
    pub failure: MotorFailure,
}

#[cfg(feature = "std")]
impl Alert {
    pub fn to_csv(&self) -> String {
        format!("{},{},{}", self.motor_id, self.time, self.failure)
    }

    pub fn from_csv(csv_line: String) -> Alert {
        let values: Vec<&str> = csv_line.split(',').collect();
        Alert {
            motor_id: u16::from_str(values[0]).expect("Could not parse motor id"),
            time: f64::from_str(values[1]).expect("Could not parse time"),
            failure: MotorFailure::from_str(values[2]).expect("Could not parse MotorFailure"),
        }
    }

    pub fn from_alert_with_delay(alert_with_delay: AlertWithDelay) -> Alert {
        Alert {
            time: alert_with_delay.time,
            motor_id: alert_with_delay.motor_id,
            failure: alert_with_delay.failure,
        }
    }
}

#[cfg(feature = "std")]
#[derive(Serialize, Deserialize, Debug)]
pub struct AlertWithDelay {
    pub time: f64,
    pub motor_id: u16,
    pub failure: MotorFailure,
    pub delay: f64,
}

#[cfg(feature = "std")]
impl AlertWithDelay {
    pub fn from_csv(csv_line: String) -> AlertWithDelay {
        let values: Vec<&str> = csv_line.split(',').collect();
        AlertWithDelay {
            motor_id: u16::from_str(values[0]).expect("Could not parse motor id"),
            time: f64::from_str(values[1]).expect("Could not parse time"),
            failure: MotorFailure::from_str(values[2]).expect("Could not parse MotorFailure"),
            delay: f64::from_str(values[3]).expect("Could not parse delay"),
        }
    }
}

#[cfg(feature = "std")]
#[derive(Serialize, Deserialize, Debug)]
pub struct CloudServerRunParameters {
    pub start_time: f64,
    pub duration: f64,
    pub motor_monitor_listen_address: SocketAddr,
    pub request_processing_model: RequestProcessingModel,
}

#[cfg(feature = "std")]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MotorSensorGroup {
    at_sensor: SocketAddr,
    pt_sensor: SocketAddr,
    rs_sensor: SocketAddr,
    tq_sensor: SocketAddr,
}

#[cfg(feature = "std")]
impl Index<usize> for MotorSensorGroup {
    type Output = SocketAddr;

    fn index(&self, index: usize) -> &Self::Output {
        match index {
            0 => &self.at_sensor,
            1 => &self.pt_sensor,
            2 => &self.rs_sensor,
            3 => &self.tq_sensor,
            _ => panic!("Invalid MotorData index"),
        }
    }
}

#[cfg(feature = "std")]
impl IntoIterator for MotorSensorGroup {
    type Item = SocketAddr;
    type IntoIter = std::array::IntoIter<Self::Item, 4>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIterator::into_iter([
            self.at_sensor,
            self.pt_sensor,
            self.rs_sensor,
            self.tq_sensor,
        ])
    }
}

#[cfg(feature = "std")]
impl<'a> IntoIterator for &'a MotorSensorGroup {
    type Item = &'a SocketAddr;
    type IntoIter = std::array::IntoIter<Self::Item, 4>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIterator::into_iter([
            &self.at_sensor,
            &self.pt_sensor,
            &self.rs_sensor,
            &self.tq_sensor,
        ])
    }
}

#[cfg(feature = "std")]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NetworkConfig {
    pub cloud_server_address: IpAddr,
    pub motor_monitor_address: IpAddr,
    pub sensor_addresses: Vec<IpAddr>,
}
