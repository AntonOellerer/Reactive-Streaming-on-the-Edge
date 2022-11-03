use std::{env, fs};

fn main() {
    // Tell Cargo that if the given file changes, to rerun this build script.
    println!("cargo:rerun-if-changed=resources/*.txt");
    let out_dir = env::var("OUT_DIR").unwrap();

    let sensor_id = env::var("SENSOR_ID").unwrap_or_else(|_| String::from("1"));
    fs::write(format!("{out_dir}/sensor_id.in",), sensor_id.clone()).unwrap();

    fs::copy(
        format!("resources/{}.txt", sensor_id),
        format!("{out_dir}/sensor_readings.txt"),
    )
    .unwrap();
}
