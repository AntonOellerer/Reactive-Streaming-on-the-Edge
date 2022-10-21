use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, thread};

use libc::time_t;
use postcard::{to_allocvec, to_allocvec_cobs};
use procfs::process::Process;
use rand::prelude::IteratorRandom;
use rand::rngs::SmallRng;
use rand::SeedableRng;

use data_transfer_objects::RequestProcessingModel::ClientServer;
use data_transfer_objects::{
    BenchmarkData, BenchmarkDataType, RequestProcessingModel, SensorMessage, SensorParameters,
};

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let data_path = get_and_validate_path(&arguments);

    let sensor_parameters: SensorParameters = get_sensor_parameters(&arguments);
    let mut rng = SmallRng::seed_from_u64(sensor_parameters.id as u64);

    if sensor_parameters.request_processing_model == ClientServer {
        execute_client_server_procedure(
            data_path,
            &sensor_parameters,
            &mut rng,
            Duration::from_secs(sensor_parameters.duration as u64),
        )
    }
    save_benchmark_readings(sensor_parameters.id);
}

fn get_and_validate_path(args: &[String]) -> &Path {
    let path = args.get(1).expect("Did not receive at least 1 argument");
    let path = Path::new(path);
    let path_valid = path.try_exists();
    assert!(path_valid.is_ok() & path_valid.expect("Invalid data file path given to sensors"));
    path
}

fn get_sensor_parameters(arguments: &[String]) -> SensorParameters {
    SensorParameters {
        id: arguments
            .get(2)
            .expect("Did not receive at least 2 arguments")
            .parse()
            .expect("Could not parse id successfully"),
        duration: arguments
            .get(3)
            .expect("Did not receive at least 4 arguments")
            .parse()
            .expect("Could not parse duration successfully"),
        sampling_interval: arguments
            .get(4)
            .expect("Did not receive at least 5 arguments")
            .parse()
            .expect("Could not parse sampling interval successfully"),
        request_processing_model: RequestProcessingModel::from_str(
            arguments
                .get(5)
                .expect("Did not receive at least 6 arguments"),
        )
        .expect("Could not parse Request Processing Model successfully"),
        motor_monitor_port: arguments
            .get(6)
            .expect("Did not receive at least 7 arguments")
            .parse()
            .expect("Could not parse port successfully"),
    }
}

fn execute_client_server_procedure(
    data_path: &Path,
    sensor_parameters: &SensorParameters,
    mut rng: &mut SmallRng,
    duration: Duration,
) {
    let end_time = get_now() + duration.as_secs() as time_t;
    while get_now() < end_time {
        let sensor_reading = fs::read(data_path)
            .expect("Failure reading sensor data")
            .lines()
            .choose_stable(&mut rng)
            .expect("Data file iterator is empty")
            .expect("Error reading from data file iterator")
            .parse()
            .expect("Error parsing data fileline");
        send_sensor_reading(sensor_parameters, sensor_reading);
        thread::sleep(Duration::from_millis(
            sensor_parameters.sampling_interval as u64,
        ))
    }
}

fn send_sensor_reading(sensor_parameters: &SensorParameters, sensor_reading: f32) {
    let message = SensorMessage {
        reading: sensor_reading,
        sensor_id: sensor_parameters.id,
    };
    match TcpStream::connect(format!(
        "localhost:{}",
        sensor_parameters.motor_monitor_port
    )) {
        Ok(mut stream) => {
            let vec: Vec<u8> =
                to_allocvec(&message).expect("Could not write sensor reading to Vec<u8>");
            stream
                .write_all(&vec)
                .expect("Could not write sensor reading bytes to TcpStream");
        }
        Err(e) => {
            eprintln!("Failed to connect: {}", e);
        }
    }
}

fn save_benchmark_readings(id: u32) {
    let me = Process::myself().expect("Could not get process info handle");
    let stat = me.stat().expect("Could not get /proc/[pid]/stat info");
    let status = me.status().expect("Could not get /proc/[pid]/status info");
    let benchmark_data = BenchmarkData {
        id,
        time_spent_in_kernel_mode: stat.stime,
        time_spent_in_user_mode: stat.utime,
        children_time_spent_in_kernel_mode: stat.cstime,
        children_time_spent_in_user_mode: stat.cutime,
        memory_high_water_mark: status.vmhwm.expect("Could not get vmhw"),
        memory_resident_set_size: status.vmrss.expect("Could not get vmrss"),
        benchmark_data_type: BenchmarkDataType::Sensor,
    };
    let vec: Vec<u8> =
        to_allocvec_cobs(&benchmark_data).expect("Could not write benchmark data to Vec<u8>");
    let _wrote = std::io::stdout()
        .write(&vec)
        .expect("Could not write benchmark data bytes to stdout");
}

pub fn get_now() -> time_t {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Could not get epoch seconds")
        .as_secs()
        .try_into()
        .expect("Could not convert now start to time_t")
}
