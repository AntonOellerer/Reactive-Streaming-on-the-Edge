use data_transfer_objects::SensorMessage;
use libc::time_t;

#[derive(Debug)]
pub struct SlidingWindow {
    window_size: i64,
    elements: Vec<SensorMessage>,
}

impl SlidingWindow {
    pub fn new(window_size: i64) -> SlidingWindow {
        SlidingWindow {
            window_size,
            elements: Vec::new(),
        }
    }

    pub fn add(&mut self, element: SensorMessage) {
        self.elements.push(element);
    }

    pub fn get_window_average(&self) -> f64 {
        let reading_sum: f64 = self
            .elements
            .iter()
            .map(|message| message.reading as f64)
            .sum();
        reading_sum / (self.elements.len() as f64)
    }

    pub fn refresh_cache(&mut self, at_time: time_t) {
        self.elements
            .retain(|message| message.timestamp > at_time - self.window_size);
    }

    pub fn reset(&mut self) {
        self.elements = Vec::new();
    }
}
