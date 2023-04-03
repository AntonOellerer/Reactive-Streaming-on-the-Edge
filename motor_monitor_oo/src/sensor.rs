use data_transfer_objects::SensorMessage;
use log::debug;
use std::net::{SocketAddr, TcpListener};
use std::sync::mpsc::Sender;
use std::time::Duration;

pub struct SensorAverage {
    pub average: f64,
    pub timestamp: f64,
    pub sensor_id: u32,
}

struct SlidingWindow {
    size: Duration,
    last_sent: Duration,
    elements: Vec<SensorMessage>,
}

impl SlidingWindow {
    fn update(&mut self) {
        let now = utils::get_now_duration();
        self.elements
            .retain(|message| now - Duration::from_secs_f64(message.timestamp) <= self.size);
    }

    fn get_window_average(&self) -> f64 {
        if self.elements.len() == 0 {
            0f64
        } else {
            let reading_sum: f64 = self
                .elements
                .iter()
                .map(|message| message.reading as f64)
                .sum();
            reading_sum / (self.elements.len() as f64)
        }
    }
}

pub struct Sensor {
    // sensor_id: u32,
    pub monitor_connection: Sender<SensorAverage>,
    pub listen_addr: SocketAddr,
    window: SlidingWindow,
}

impl Sensor {
    pub fn build(
        window_size: Duration,
        monitor_connection: Sender<SensorAverage>,
        listen_addr: SocketAddr,
    ) -> Sensor {
        Sensor {
            monitor_connection,
            listen_addr,
            window: SlidingWindow {
                size: window_size,
                last_sent: utils::get_now_duration(),
                elements: vec![],
            },
        }
    }
    pub fn run(mut self) {
        let listener = TcpListener::bind(self.listen_addr).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("Could not set read timeout");
        while let Some(sensor_message) = utils::read_object::<SensorMessage>(&mut stream) {
            self.handle_sensor_message(sensor_message);
        }
    }

    fn handle_sensor_message(&mut self, message: SensorMessage) {
        debug!("{message:?}");
        self.window.elements.push(message);
        let now = utils::get_now_duration();
        if now - self.window.last_sent > self.window.size {
            self.window.update();
            self.monitor_connection
                .send(SensorAverage {
                    average: self.window.get_window_average(),
                    timestamp: message.timestamp,
                    sensor_id: message.sensor_id,
                })
                .unwrap();
            self.window.last_sent = now;
        }
    }
}
