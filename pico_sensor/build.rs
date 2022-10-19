use std::{env, fs};

fn main() {
    // Tell Cargo that if the given file changes, to rerun this build script.
    println!("cargo:rerun-if-changed=resources/*.txt");
    // Use the `cc` crate to build a C file and statically link it.
    fs::copy(
        format!("resources/{}.txt", env!("SENSOR_ID")),
        format!("{}/{}", env::var("OUT_DIR").unwrap(), "sensor_readings.txt"),
    )
    .unwrap();
}
