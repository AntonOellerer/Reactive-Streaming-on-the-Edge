use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fs, thread};

use data_transfer_objects::RequestProcessingModel::ClientServer;
use data_transfer_objects::{
    RequestProcessingModel, SensorBenchmarkData, SensorMessage, SensorParameters,
};
use libc::time_t;
use postcard::to_allocvec;
use procfs::process::Process;
use rand::prelude::IteratorRandom;
use rand::rngs::SmallRng;
use rand::SeedableRng;

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let data_path = get_and_validate_path(&arguments);

    let sensor_parameters: SensorParameters = get_sensor_parameters(&arguments);
    let mut rng = SmallRng::seed_from_u64(sensor_parameters.seed as u64);

    let (start_time, end_time) = get_times(&sensor_parameters);
    thread::sleep(start_time - Instant::now());
    if sensor_parameters.request_processing_model == ClientServer {
        execute_client_server_procedure(data_path, &sensor_parameters, &mut rng, &end_time)
    }
    save_benchmark_readings();
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
        start_time: arguments
            .get(3)
            .expect("Did not receive at least 3 arguments")
            .parse()
            .expect("Could not parse start time successfully"),
        duration: arguments
            .get(4)
            .expect("Did not receive at least 4 arguments")
            .parse()
            .expect("Could not parse duration successfully"),
        seed: arguments
            .get(5)
            .expect("Did not receive at least 5 arguments")
            .parse()
            .expect("Could not parse seed successfully"),
        sampling_interval: arguments
            .get(6)
            .expect("Did not receive at least 6 arguments")
            .parse()
            .expect("Could not parse sampling interval successfully"),
        request_processing_model: RequestProcessingModel::from_str(
            arguments
                .get(7)
                .expect("Did not receive at least 7 arguments"),
        )
        .expect("Could not parse Request Processing Model successfully"),
        port: arguments
            .get(8)
            .expect("Did not receive at least 8 arguments")
            .parse()
            .expect("Could not parse port successfully"),
    }
}

fn get_times(sensor_parameters: &SensorParameters) -> (Instant, Instant) {
    let start_time = Instant::now() + Duration::new(sensor_parameters.start_time as u64, 0);
    let end_time = start_time + Duration::new(sensor_parameters.duration, 0);
    (start_time, end_time)
}

fn execute_client_server_procedure(
    data_path: &Path,
    sensor_parameters: &SensorParameters,
    mut rng: &mut SmallRng,
    end_time: &Instant,
) {
    while Instant::now() < *end_time {
        let sensor_reading = fs::read(data_path)
            .expect("Failure reading sensor data")
            .lines()
            .choose_stable(&mut rng)
            .expect("Data file iterator is empty")
            .expect("Error reading from data file iterator")
            .parse()
            .expect("Error parsing data fileline");
        send_sensor_reading(sensor_parameters, sensor_reading);
        thread::sleep(Duration::from_micros(sensor_parameters.sampling_interval as u64))
    }
}

fn send_sensor_reading(sensor_parameters: &SensorParameters, sensor_reading: f32) {
    let message = SensorMessage {
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Could not get the duration since unix epoch")
            .as_secs() as time_t,
        reading: sensor_reading,
        sensor_id: sensor_parameters.id,
    };
    match TcpStream::connect(format!("localhost:{}", sensor_parameters.port)) {
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

fn save_benchmark_readings() {
    let me = Process::myself().expect("Could not get process info handle");
    let stat = me.stat().expect("Could not get /proc/[pid]/stat info");
    let status = me.status().expect("Could not get /proc/[pid]/status info");
    let benchmark_data = SensorBenchmarkData {
        time_spent_in_kernel_mode: stat.stime,
        time_spent_in_user_mode: stat.utime,
        children_time_spent_in_kernel_mode: stat.cstime,
        children_time_spent_in_user_mode: stat.cutime,
        memory_high_water_mark: status.vmhwm.expect("Could not get vmhw"),
        memory_resident_set_size: status.vmrss.expect("Could not get vmrss"),
    };
    let vec: Vec<u8> =
        to_allocvec(&benchmark_data).expect("Could not write benchmark data to Vec<u8>");
    let _read = std::io::stdout()
        .write(&vec)
        .expect("Could not write benchmark data bytes to stdout");
}
