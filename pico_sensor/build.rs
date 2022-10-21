use std::{env, fs};

fn main() {
    // Tell Cargo that if the given file changes, to rerun this build script.
    println!("cargo:rerun-if-changed=resources/*.txt");
    let out_dir = env::var("OUT_DIR").unwrap();

    fs::write(format!("{}/{}", out_dir, "sensor_id.in"), env!("SENSOR_ID")).unwrap();

    fs::copy(
        format!("resources/{}.txt", env!("SENSOR_ID")),
        format!("{}/{}", out_dir, "sensor_readings.txt"),
    )
    .unwrap();
}
