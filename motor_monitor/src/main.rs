use std::io::Read;
use std::mem::size_of;
use std::net::{TcpListener, TcpStream};
use std::ops::{BitAnd, Index, IndexMut, Shr};
use std::str::FromStr;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use data_transfer_objects::{MotorMonitorParameters, RequestProcessingModel, SensorMessage};
use postcard::from_bytes;
use threadpool::ThreadPool;

use crate::motor_sensor_group_buffers::MotorGroupSensorsBuffers;
use crate::rules_engine::violated_rule;
use crate::sliding_window::SlidingWindow;

mod motor_sensor_group_buffers;
mod rules_engine;
mod sliding_window;
mod util;

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

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let motor_monitor_parameters: MotorMonitorParameters = get_motor_monitor_parameters(&arguments);
    let end_time = Instant::now()
        + Duration::from_secs(
            motor_monitor_parameters
                .start_time
                .try_into()
                .expect("Could not convert time_t start time to i64 start time"),
        )
        + Duration::from_secs(motor_monitor_parameters.duration);
    let (tx, rx) = channel();
    let pool = ThreadPool::new(8);
    setup_receivers(&motor_monitor_parameters, tx, &pool);
    let consumer_thread = setup_consumer(rx, end_time, motor_monitor_parameters);
    thread::sleep(end_time - Instant::now());
    drop(pool);
    drop(consumer_thread);
}

fn get_motor_monitor_parameters(arguments: &[String]) -> MotorMonitorParameters {
    MotorMonitorParameters {
        start_time: arguments
            .get(1)
            .expect("Did not receive at least 2 arguments")
            .parse()
            .expect("Could not parse start_time successfully"),
        duration: arguments
            .get(2)
            .expect("Did not receive at least 3 arguments")
            .parse()
            .expect("Could not parse duration successfully"),
        request_processing_model: RequestProcessingModel::from_str(
            arguments
                .get(3)
                .expect("Did not receive at least 4 arguments"),
        )
        .expect("Could not parse Request Processing Model successfully"),
        number_of_motor_groups: arguments
            .get(4)
            .expect("Did not receive at least 5 arguments")
            .parse()
            .expect("Could not parse number_of_motor_groups successfully"),
        window_size: arguments
            .get(5)
            .expect("Did not receive at least 6 arguments")
            .parse()
            .expect("Could not parse window_size successfully"),
        start_port: arguments
            .get(6)
            .expect("Did not receive at least 7 arguments")
            .parse()
            .expect("Could not parse start_port successfully"),
    }
}

fn setup_receivers(args: &MotorMonitorParameters, tx: Sender<SensorMessage>, pool: &ThreadPool) {
    for port in args.start_port..=args.start_port + args.number_of_motor_groups as u16 * 4 {
        let tx = tx.clone();
        let listener = TcpListener::bind(format!("localhost:{}", port))
            .expect(&*format!("Could not bind sensor data listener to {}", port));
        pool.execute(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        handle_sensor_message(&tx, stream);
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        /* connection failed */
                    }
                }
            }
        });
    }
}

fn handle_sensor_message(tx: &Sender<SensorMessage>, mut stream: TcpStream) {
    let mut data = [0; size_of::<SensorMessage>()];
    let _read = stream
        .read(&mut data)
        .expect("Could not read sensor data bytes from stream");
    let result: SensorMessage =
        from_bytes(&data).expect("Could not parse sensor data bytes to SensorMessage");
    let _ = tx.send(result);
}

fn setup_consumer(
    rx: Receiver<SensorMessage>,
    end_time: Instant,
    motor_monitor_parameters: MotorMonitorParameters,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut buffers: Vec<MotorGroupSensorsBuffers> =
            Vec::with_capacity(motor_monitor_parameters.number_of_motor_groups);
        for _ in 0..motor_monitor_parameters.number_of_motor_groups {
            buffers.push(MotorGroupSensorsBuffers::new(
                motor_monitor_parameters.window_size,
            ))
        }
        while Instant::now() < end_time {
            let message = rx.recv();
            match message {
                Ok(message) => {
                    handle_message(&mut buffers, message);
                }
                Err(e) => eprintln!("Error: {}", e),
            };
        }
    })
}

fn handle_message(buffers: &mut [MotorGroupSensorsBuffers], message: SensorMessage) {
    let motor_group_id: u32 = message.sensor_id.shr(u32::BITS / 2);
    let sensor_id = message.sensor_id.bitand(0xFFFF);
    println!(
        "Received message from {} ({} {}): {}",
        message.sensor_id, motor_group_id, sensor_id, message.reading
    );
    let motor_group = buffers
        .get_mut(usize::try_from(motor_group_id).expect("Could not convert u32 id to usize"))
        .expect("Motor group id did not match to a motor group buffer");
    let sensor_buffer =
        &mut motor_group[usize::try_from(sensor_id).expect("Could not convert u32 id to usize")];
    sensor_buffer.add(message);
    let now = util::get_now();
    motor_group.refresh_caches(now);
    let rule_violated = violated_rule(motor_group);
    if let Some(rule) = rule_violated {
        println!("Found rule violation {} in motor {}", rule, motor_group_id);
        motor_group.reset();
    }
}
