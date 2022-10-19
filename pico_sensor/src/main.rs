#![no_main]
#![no_std]

use core::mem::size_of;

// Ensure we halt the program on panic (if we don't mention this crate it won't
// be linked)
use panic_halt as _;
use rand::prelude::{IteratorRandom, SmallRng};
use rand::SeedableRng;
// The macro for our start-up function
use rp_pico::entry;
// A shorter alias for the Hardware Abstraction Layer, which provides
// higher-level drivers.
use rp_pico::hal;
// A shorter alias for the Peripheral Access Crate, which provides low-level
// register access
use rp_pico::hal::pac;
// Pull in any important traits
use rp_pico::hal::prelude::*;

use data_transfer_objects::{SensorMessage, SensorParameters};

const SENSOR_READINGS: &str = include_str!(concat!(env!("OUT_DIR"), "/sensor_readings.txt"));

#[entry]
fn main() -> ! {
    // Grab our singleton objects
    let mut pac = pac::Peripherals::take().unwrap();
    let core = pac::CorePeripherals::take().unwrap();

    // Set up the watchdog driver - needed by the clock setup code
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

    // Configure the clocks
    //
    // The default is to generate a 125 MHz system clock
    let clocks = hal::clocks::init_clocks_and_plls(
        rp_pico::XOSC_CRYSTAL_FREQ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    // The single-cycle I/O block controls our GPIO pins
    let sio = hal::Sio::new(pac.SIO);

    // Set the pins up according to their function on this particular board
    let pins = rp_pico::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // Configure two pins as being I²C, not GPIO
    let sda_pin = pins.gpio16.into_mode::<hal::gpio::FunctionI2C>();
    let scl_pin = pins.gpio17.into_mode::<hal::gpio::FunctionI2C>();

    // Create the I²C driver, using the two pre-configured pins. This will fail
    // at compile time if the pins are in the wrong mode, or if this I²C
    // peripheral isn't available on these pins!
    let mut i2c =
        hal::I2C::new_peripheral_event_iterator(pac.I2C0, sda_pin, scl_pin, &mut pac.RESETS, 1u16);
    // The delay object lets us wait for specified amounts of time (in
    // milliseconds)
    let mut delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());
    loop {
        let mut sensor_parameters_buffer = [0; size_of::<SensorParameters>()];
        i2c.read(&mut sensor_parameters_buffer);
        let sensor_parameters =
            postcard::from_bytes_cobs::<SensorParameters>(&mut sensor_parameters_buffer)
                .expect("Could not decode parameters");
        let start_instant = fugit::TimerInstantU32::<1_000_000>::from_ticks(0);
        let mut rng = SmallRng::seed_from_u64(sensor_parameters.id as u64);
        let mut message_buffer = [0u8; 32];
        while start_instant.duration_since_epoch().to_secs() < sensor_parameters.duration {
            let sensor_reading: f32 = SENSOR_READINGS
                .lines()
                .choose_stable(&mut rng)
                .expect("Data file iterator is empty")
                .parse()
                .expect("Error parsing data file line");
            let message_bytes = postcard::to_slice_cobs(
                &SensorMessage {
                    reading: sensor_reading,
                    sensor_id: sensor_parameters.id,
                },
                &mut message_buffer,
            )
            .expect("Could not encode sensor message to vector");
            let mut i = 0;
            while i < message_bytes.len() {
                i += i2c.write(&message_bytes[i..]);
            }
            delay.delay_ms(sensor_parameters.sampling_interval);
        }
    }
}
