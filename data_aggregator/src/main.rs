use polars::datatypes::DataType;
use polars::frame::DataFrame;
use polars::prelude::SerReader;
use polars::prelude::{ChunkVar, Series};
use polars::prelude::{CsvReader, Schema};
use std::fs;
use std::fs::{read_dir, DirEntry};
use std::str::FromStr;

const RAW_DATA_PATH: &str = "../bench_executor/";

fn main() {
    let processing_models = vec!["ClientServer", "ReactiveStreaming"];
    for processing_model in processing_models {
        aggregate_processing_time(processing_model);
        aggregate_memory_usage(processing_model);
        aggregate_alert_delays(processing_model);
        aggregate_alert_failures(processing_model);
    }
}

fn aggregate_processing_time(processing_model: &str) {
    let mut aggregates: Vec<(i32, f64, f64, f64, f64)> = vec![];
    let data_frames = get_data_frames(processing_model, "ru");
    for (file_name, data_frame) in data_frames {
        let no_motor_groups = get_number_of_motor_groups(file_name);
        let total_time = &(&(&data_frame["utime"] + &data_frame["stime"]) + &data_frame["cutime"])
            + &data_frame["cstime"];
        append_aggregates(&mut aggregates, no_motor_groups, &total_time);
    }
    write_results_as_csv(&mut aggregates, format!("{processing_model}_time.csv"));
}

fn aggregate_memory_usage(processing_model: &str) {
    let mut aggregates: Vec<(i32, f64, f64, f64, f64)> = vec![];
    let data_frames = get_data_frames(processing_model, "ru");
    for (file_name, data_frame) in data_frames {
        let no_motor_groups = get_number_of_motor_groups(file_name);
        let vmhwm = &data_frame["vmhwm"];
        append_aggregates(&mut aggregates, no_motor_groups, vmhwm);
    }
    write_results_as_csv(&mut aggregates, format!("{processing_model}_memory.csv"));
}

fn aggregate_alert_delays(processing_model: &str) {
    let mut aggregates: Vec<(i32, f64, f64, f64, f64)> = vec![];
    let dir_entries = get_relevant_files(processing_model, "ad");
    for dir_entry in dir_entries {
        let no_motor_groups =
            get_number_of_motor_groups(dir_entry.file_name().into_string().unwrap());
        let series = read_csv_to_series(dir_entry);
        append_aggregates(&mut aggregates, no_motor_groups, &series);
    }
    write_results_as_csv(&mut aggregates, format!("{processing_model}_delays.csv"));
}

fn aggregate_alert_failures(processing_model: &str) {
    let mut aggregates: Vec<(i32, f64, f64, f64, f64)> = vec![];
    let dir_entries = get_relevant_files(processing_model, "af");
    for dir_entry in dir_entries {
        let no_motor_groups =
            get_number_of_motor_groups(dir_entry.file_name().into_string().unwrap());
        let series = read_csv_to_series(dir_entry);
        append_aggregates(&mut aggregates, no_motor_groups, &series);
    }
    write_results_as_csv(&mut aggregates, format!("{processing_model}_failures.csv"));
}

fn read_csv_to_series(dir_entry: DirEntry) -> Series {
    let series: Series = fs::read_to_string(dir_entry.path())
        .expect("Alert delay file should be readable to string")
        .split(',')
        .filter(|token| !token.is_empty())
        .map(f64::from_str)
        .map(Result::unwrap)
        .collect();
    series
}

fn get_number_of_motor_groups(file_name: String) -> i32 {
    let motor_groups = file_name
        .split('_')
        .next()
        .expect("Resource usage file should start with the number of motor groups")
        .parse::<i32>()
        .expect("First number of file should be parsable to number of motor groups");
    motor_groups
}

fn append_aggregates(
    aggregates: &mut Vec<(i32, f64, f64, f64, f64)>,
    no_motor_groups: i32,
    series: &Series,
) {
    let std = match series.f64() {
        Ok(cast) => cast.std(1).unwrap_or(0.0),
        Err(_) => match series.i64() {
            Ok(cast) => cast.std(1).unwrap_or(0.0),
            Err(_) => 0.0,
        },
    };
    aggregates.push((
        no_motor_groups,
        series.min().unwrap_or(0.0),
        series.mean().unwrap_or(0.0),
        series.max().unwrap_or(0.0),
        std,
    ));
}

fn write_results_as_csv(aggregates: &mut [(i32, f64, f64, f64, f64)], file_name: String) {
    aggregates.sort_by_key(|agg| agg.0);
    let result = aggregates
        .iter()
        .map(|(id, min, mean, max, std)| format!("{id},{min},{mean},{max},{std}"))
        .collect::<Vec<String>>()
        .join("\n");
    std::fs::write(file_name, format!("no,min,mean,max,std\n{result}"))
        .expect("Should be able to write processing model results");
}

fn get_data_frames(processing_model: &str, file_name_marker: &str) -> Vec<(String, DataFrame)> {
    let mut schema = Schema::new();
    schema.with_column("id".to_string(), DataType::Int64);
    schema.with_column("utime".to_string(), DataType::Int64);
    schema.with_column("stime".to_string(), DataType::Int64);
    schema.with_column("cutime".to_string(), DataType::Int64);
    schema.with_column("cstime".to_string(), DataType::Int64);
    schema.with_column("vmhwm".to_string(), DataType::Int64);
    schema.with_column("vmpeak".to_string(), DataType::Int64);

    get_relevant_files(processing_model, file_name_marker)
        .iter()
        .map(|dir_entry| {
            (
                dir_entry
                    .file_name()
                    .into_string()
                    .expect("Result file should have UTF-8 name"),
                CsvReader::from_path(dir_entry.path())
                    .map(|csv_reader| {
                        csv_reader
                            .has_header(true)
                            .with_dtypes(Some(&schema))
                            .finish()
                            .expect("Result file should be readable as csv")
                    })
                    .expect("Result file should be readable as data frame"),
            )
        })
        .collect::<Vec<(String, DataFrame)>>()
}

fn get_relevant_files(processing_model: &str, file_name_marker: &str) -> Vec<DirEntry> {
    read_dir(RAW_DATA_PATH)
        .expect("Raw data directory should exist and be readable")
        .filter_map(|dir_entry| dir_entry.ok())
        .filter_map(|dir_entry| {
            if let Ok(file_name) = dir_entry.file_name().into_string() {
                if file_name.contains(processing_model) && file_name.contains(file_name_marker) {
                    return Some(dir_entry);
                }
            }
            None
        })
        .collect()
}
