use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::ops::{BitAnd, Shl, Shr};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use chrono::NaiveDateTime;
use env_logger::Target;
use futures::executor::{ThreadPool, ThreadPoolBuilder};
use futures::future::RemoteHandle;
use log::{debug, error, info};
use postcard::to_allocvec_cobs;
use springql::{SpringConfig, SpringPipeline, SpringSinkRow};

use data_transfer_objects::{Alert, BenchmarkDataType, MotorFailure, MotorMonitorParameters};
use scheduler::Scheduler;

#[derive(Debug, Copy, Clone, Default)]
struct MotorData {
    timestamp: f64,
    motor_id: u32,
    temperature_difference: Option<f32>,
    rotational_speed: Option<f32>,
    power: Option<f32>,
    torque: Option<f32>,
}

#[cfg(debug_assertions)]
const POST_MONITORING: bool = true;
#[cfg(not(debug_assertions))]
const POST_MONITORING: bool = false;

impl MotorData {
    fn from_springql_row(row: SpringSinkRow) -> MotorData {
        MotorData {
            timestamp: Self::get_timestamp_f64(&row),
            motor_id: row
                .get_not_null_by_index(1)
                .expect("Could not get motor_id"),
            temperature_difference: row.get_not_null_by_index::<f32>(2).ok(),
            rotational_speed: row.get_not_null_by_index::<f32>(3).ok(),
            power: row.get_not_null_by_index::<f32>(4).ok(),
            torque: row.get_not_null_by_index::<f32>(5).ok(),
        }
    }

    fn is_some(&self) -> bool {
        self.temperature_difference.is_some()
            && self.power.is_some()
            && self.rotational_speed.is_some()
            && self.torque.is_some()
    }

    fn get_timestamp_f64(row: &SpringSinkRow) -> f64 {
        Duration::from_millis(
            NaiveDateTime::parse_from_str(
                row.get_not_null_by_index::<String>(0)
                    .expect("Could not get timestamp")
                    .as_str(),
                // format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:9]")
                "%Y-%m-%d %H:%M:%S%.9f",
            )
            .expect("Could not parse timestamp")
            .timestamp_millis() as u64,
        )
        .as_secs_f64()
    }
}

fn main() {
    env_logger::builder().target(Target::Stderr).init();
    let arguments: Vec<String> = std::env::args().collect();
    let motor_monitor_parameters: MotorMonitorParameters =
        utils::get_motor_monitor_parameters(&arguments);
    info!("Running procedure");
    execute_procedure(motor_monitor_parameters);
    info!("Processing completed");
    utils::save_benchmark_readings(0, BenchmarkDataType::MotorMonitor);
    info!("Saved benchmark readings");
}

fn execute_procedure(motor_monitor_parameters: MotorMonitorParameters) {
    let pool = ThreadPoolBuilder::new()
        .pool_size(motor_monitor_parameters.thread_pool_size)
        .create()
        .unwrap();
    let pipeline = setup_processing_pipeline(motor_monitor_parameters);
    let handle_list = evaluate_results(pipeline, motor_monitor_parameters, pool);
    wait_on_complete(handle_list);
}

fn setup_processing_pipeline(
    motor_monitor_parameters: MotorMonitorParameters,
) -> Arc<SpringPipeline> {
    let mut config = SpringConfig::default();
    config.web_console.enable_report_post = POST_MONITORING;
    config.worker.n_source_worker_threads =
        motor_monitor_parameters.number_of_tcp_motor_groups as u16 * 4; // one per source
    config.worker.n_generic_worker_threads =
        motor_monitor_parameters.thread_pool_size as u16 - config.worker.n_source_worker_threads; // rest for the other tasks
    let pipeline = Arc::new(SpringPipeline::new(&config).unwrap());
    for motor_id in 0..motor_monitor_parameters.number_of_tcp_motor_groups {
        pipeline
            .command(format!(
                "
                CREATE SINK STREAM motor_averages_{motor_id} (
                    min_ts TIMESTAMP NOT NULL ROWTIME,
                    motor_id INTEGER NOT NULL,
                    temperature_difference FLOAT,
                    rotational_speed FLOAT,
                    power FLOAT,
                    torque FLOAT
                );
                ",
            ))
            .unwrap();
        for sensor_id in 0..=3 {
            let full_id: u32 = (motor_id as u32).shl(2) + sensor_id as u32;
            pipeline
                .command(format!(
                    "
                    CREATE SOURCE STREAM sensor_data_{motor_id}_{sensor_id} (
                        ts TIMESTAMP NOT NULL ROWTIME,
                        sensor_id INTEGER NOT NULL,
                        reading FLOAT NOT NULL
                    );
                    ",
                ))
                .unwrap();

            pipeline
                .command(format!(
                    "
                    CREATE SOURCE READER sensor_data_reader_{motor_id}_{sensor_id} FOR sensor_data_{motor_id}_{sensor_id}
                        TYPE NET_SERVER OPTIONS (
                        PROTOCOL 'TCP',
                        PORT '{}'
                    );
                    ", motor_monitor_parameters.sensor_listen_address.port() + full_id as u16))
                .unwrap();

            pipeline
                .command(format!(
                    "
                    CREATE STREAM sensor_average_{motor_id}_{sensor_id} (
                        min_ts TIMESTAMP NOT NULL ROWTIME,
                        sensor_id INTEGER NOT NULL,
                        avg_reading FLOAT NOT NULL
                    );
                    "
                ))
                .unwrap();

            pipeline
                .command(format!(
                    "
                    CREATE PUMP pump_sensor_average_{motor_id}_{sensor_id} AS
                    INSERT INTO sensor_average_{motor_id}_{sensor_id} (min_ts, sensor_id, avg_reading)
                    SELECT STREAM
                        FLOOR_TIME(sensor_data_{motor_id}_{sensor_id}.ts, DURATION_MILLIS({})) AS min_ts,
                        sensor_data_{motor_id}_{sensor_id}.sensor_id AS sensor_id,
                        AVG(sensor_data_{motor_id}_{sensor_id}.reading) AS avg_reading
                    FROM sensor_data_{motor_id}_{sensor_id}
                    GROUP BY min_ts, sensor_id
                    SLIDING WINDOW DURATION_MILLIS({}), DURATION_MILLIS({}), DURATION_MILLIS(0);
                    ",
                    motor_monitor_parameters.window_size_ms,
                    motor_monitor_parameters.window_size_ms,
                    motor_monitor_parameters.window_sampling_interval
                ))
                .unwrap()
        }

        pipeline
            .command(format!(
                "CREATE STREAM sensor_data_joined_{motor_id}_0_1 (
                    min_ts TIMESTAMP NOT NULL ROWTIME,
                    motor_id INTEGER NOT NULL,
                    temperature_difference FLOAT
                )"
            ))
            .unwrap();

        pipeline
            .command(format!(
                "
                CREATE PUMP sensor_join_values_{motor_id}_0_1 AS
                    INSERT INTO sensor_data_joined_{motor_id}_0_1 (min_ts, motor_id, temperature_difference)
                    SELECT STREAM
                        sensor_average_{motor_id}_0.min_ts,
                        {motor_id},
                        sensor_average_{motor_id}_0.avg_reading + -sensor_average_{motor_id}_1.avg_reading
                    FROM sensor_average_{motor_id}_0
                    LEFT OUTER JOIN sensor_average_{motor_id}_1
                        ON sensor_average_{motor_id}_0.min_ts = sensor_average_{motor_id}_1.min_ts
                    SLIDING WINDOW DURATION_MILLIS({}), DURATION_MILLIS({}), DURATION_MILLIS(0);
                    ",
                motor_monitor_parameters.window_size_ms,
                motor_monitor_parameters.window_sampling_interval))
            .unwrap();

        pipeline
            .command(format!(
                "CREATE STREAM sensor_data_joined_{motor_id}_2_3 (
                    min_ts TIMESTAMP NOT NULL ROWTIME,
                    motor_id INTEGER NOT NULL,
                    rotational_speed FLOAT,
                    power FLOAT,
                    torque FLOAT
                )"
            ))
            .unwrap();

        pipeline
            .command(format!(
                "
                CREATE PUMP sensor_join_values_{motor_id}_2_3 AS
                    INSERT INTO sensor_data_joined_{motor_id}_2_3 (min_ts, motor_id, rotational_speed, power, torque)
                    SELECT STREAM
                        sensor_average_{motor_id}_2.min_ts,
                        {motor_id},
                        sensor_average_{motor_id}_2.avg_reading,
                        sensor_average_{motor_id}_2.avg_reading * sensor_average_{motor_id}_3.avg_reading,
                        sensor_average_{motor_id}_3.avg_reading
                    FROM sensor_average_{motor_id}_2
                    LEFT OUTER JOIN sensor_average_{motor_id}_3
                        ON sensor_average_{motor_id}_2.min_ts = sensor_average_{motor_id}_3.min_ts
                    SLIDING WINDOW DURATION_MILLIS({}), DURATION_MILLIS({}), DURATION_MILLIS(0);
                    ",
                motor_monitor_parameters.window_size_ms,
                motor_monitor_parameters.window_sampling_interval))
            .unwrap();

        pipeline
            .command(format!(
                "
                CREATE PUMP window_avg_values_{motor_id} AS
                    INSERT INTO motor_averages_{motor_id} (min_ts, motor_id, temperature_difference, rotational_speed, power, torque)
                    SELECT STREAM
                        sensor_data_joined_{motor_id}_0_1.min_ts,
                        {motor_id},
                        sensor_data_joined_{motor_id}_0_1.temperature_difference,
                        sensor_data_joined_{motor_id}_2_3.rotational_speed,
                        sensor_data_joined_{motor_id}_2_3.power,
                        sensor_data_joined_{motor_id}_2_3.torque
                    FROM sensor_data_joined_{motor_id}_0_1
                    LEFT OUTER JOIN sensor_data_joined_{motor_id}_2_3
                        ON sensor_data_joined_{motor_id}_0_1.min_ts = sensor_data_joined_{motor_id}_2_3.min_ts
                    SLIDING WINDOW DURATION_MILLIS({}), DURATION_MILLIS({}), DURATION_MILLIS(0);
                    ",
                motor_monitor_parameters.window_size_ms,
                motor_monitor_parameters.window_sampling_interval))
            .unwrap();

        pipeline
            .command(format!(
                "
                CREATE SINK WRITER queue_writer_{motor_id} FOR motor_averages_{motor_id}
                TYPE IN_MEMORY_QUEUE OPTIONS (
                    NAME 'motor_averages_{motor_id}'
                );
            ",
            ))
            .unwrap();
    }
    pipeline
}

fn evaluate_results(
    pipeline: Arc<SpringPipeline>,
    motor_monitor_parameters: MotorMonitorParameters,
    pool: ThreadPool,
) -> Vec<RemoteHandle<()>> {
    let cloud_server = TcpStream::connect(motor_monitor_parameters.motor_monitor_listen_address)
        .expect("Could not open connection to cloud server");
    let mut handle_list = Vec::new();
    for motor_id in 0..motor_monitor_parameters.number_of_tcp_motor_groups {
        let cloud_server = cloud_server
            .try_clone()
            .expect("Could not clone TCP stream");
        let pipeline = pipeline.clone();
        handle_list.push(pool.schedule(move || {
            handle_pipeline_output(
                motor_id,
                pipeline.clone(),
                &motor_monitor_parameters,
                cloud_server,
            )
        }))
    }
    handle_list
}

fn handle_pipeline_output(
    motor_id: usize,
    pipeline: Arc<SpringPipeline>,
    motor_monitor_parameters: &MotorMonitorParameters,
    mut cloud_server: TcpStream,
) {
    let end_time = Duration::from_secs_f64(motor_monitor_parameters.start_time)
        + Duration::from_secs_f64(motor_monitor_parameters.duration);
    let mut motor_age = utils::get_now_duration();
    let mut last_message = 0f64;
    loop {
        loop {
            match pipeline.pop_non_blocking(format!("motor_averages_{motor_id}").as_str()) {
                Ok(Some(row)) => {
                    let motor_data = MotorData::from_springql_row(row);
                    if last_message != motor_data.timestamp {
                        last_message = motor_data.timestamp;
                        motor_age = handle_row(
                            motor_data,
                            motor_age,
                            &mut cloud_server,
                            motor_monitor_parameters.window_size_ms,
                        );
                    }
                }
                Err(e) => error!("{e}"),
                _ => break,
            }
        }
        thread::sleep(Duration::from_millis(
            (motor_monitor_parameters.sensor_sampling_interval / 2) as u64,
        ));
        if utils::get_now_duration() >= end_time {
            return;
        }
    }
}

fn handle_row(
    motor_data: MotorData,
    motor_age: Duration,
    cloud_server: &mut TcpStream,
    window_size: u64,
) -> Duration {
    debug!("{motor_data:?}");
    if motor_data.is_some() {
        if let Some(motor_failure) = utils::relevant_data_indicates_failure(
            motor_data.temperature_difference.unwrap() as f64,
            motor_data.rotational_speed.unwrap() as f64,
            motor_data.power.unwrap() as f64,
            motor_data.torque.unwrap() as f64
                * (utils::get_now_duration() - motor_age).as_secs_f64(),
        ) {
            send_motor_alert(motor_failure, motor_data, cloud_server, window_size);
            let now = utils::get_now_duration();
            return now;
        }
    }
    motor_age
}

fn send_motor_alert(
    motor_failure: MotorFailure,
    motor_data: MotorData,
    cloud_server: &mut TcpStream,
    window_size: u64,
) {
    let alert = Alert {
        time: motor_data.timestamp,
        motor_id: motor_data.motor_id as u16,
        failure: motor_failure,
    };
    info!("{alert:?}");
    let vec: Vec<u8> =
        to_allocvec_cobs(&alert).expect("Could not write motor monitor alert to Vec<u8>");
    let _ = cloud_server.write_all(&vec);
    debug!("Sent alert to server");
}

fn wait_on_complete(handle_list: Vec<RemoteHandle<()>>) {
    for handle in handle_list {
        futures::executor::block_on(handle);
    }
}
