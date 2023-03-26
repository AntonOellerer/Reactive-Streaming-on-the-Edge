use data_transfer_objects::SensorMessage;
use std::time::Duration;

#[derive(Debug)]
pub struct SlidingWindow {
    window_size: Duration,
    elements: Vec<SensorMessage>,
}

impl SlidingWindow {
    pub fn new(window_size: Duration) -> SlidingWindow {
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

    pub fn refresh_cache(&mut self, at_time: Duration) {
        self.elements.retain(|message| {
            Duration::from_secs_f64(message.timestamp) > at_time - self.window_size
        });
    }

    pub fn reset(&mut self) {
        self.elements = Vec::new();
    }

    pub fn iter(&self) -> impl Iterator<Item = &SensorMessage> {
        self.elements.iter()
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }
}

impl IntoIterator for SlidingWindow {
    type Item = SensorMessage;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.into_iter()
    }
}
