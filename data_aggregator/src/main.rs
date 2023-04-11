use std::fs;
use std::fs::{read_dir, DirEntry};
use std::ops::Range;
use std::str::FromStr;
use std::sync::Arc;

use plotters::prelude::{
    ChartBuilder, ErrorBar, IntoDrawingArea, IntoLogRange, PathElement, SVGBackend, BLACK, BLUE,
    GREEN, RED, WHITE,
};
use polars::datatypes::DataType;
use polars::frame::DataFrame;
use polars::prelude::SerReader;
use polars::prelude::{ChunkVar, Series};
use polars::prelude::{CsvReader, Schema};

use data_transfer_objects::RequestProcessingModel;

const RAW_DATA_PATH: &str = "../bench_executor/";
const X_LABEL: &'static str = "Window Size";

type ResultVector = Vec<(i32, f64, f64, f64, f64)>;

fn main() {
    let processing_models = vec![
        RequestProcessingModel::ObjectOriented,
        RequestProcessingModel::ReactiveStreaming,
    ];
    aggregate_processing_time(&processing_models);
    aggregate_memory_usage(&processing_models);
    aggregate_alert_delays(&processing_models);
    aggregate_alert_failures(&processing_models);
}

fn aggregate_processing_time(processing_models: &Vec<RequestProcessingModel>) {
    let mut results = vec![];
    for processing_model in processing_models {
        let mut aggregates: ResultVector = vec![];
        let data_frames = get_data_frames(processing_model, "ru");
        for (file_name, data_frame) in data_frames {
            let independent_variable = get_independent_variable(file_name);
            let total_time = &(&(&data_frame["utime"] + &data_frame["stime"])
                + &data_frame["cutime"])
                + &data_frame["cstime"];
            append_aggregates(&mut aggregates, independent_variable, &total_time);
        }
        write_results_as_csv(
            &mut aggregates,
            format!("{}_time.csv", processing_model.to_string()),
        );
        results.push((*processing_model, aggregates));
    }
    plot_aggregate_data("processing_time".to_string(), results);
}

fn aggregate_memory_usage(processing_models: &Vec<RequestProcessingModel>) {
    let mut results = vec![];
    for processing_model in processing_models {
        let mut aggregates: ResultVector = vec![];
        let data_frames = get_data_frames(processing_model, "ru");
        for (file_name, data_frame) in data_frames {
            let independet_variable = get_independent_variable(file_name);
            let vmhwm = &data_frame["vmhwm"];
            append_aggregates(&mut aggregates, independet_variable, vmhwm);
        }
        write_results_as_csv(
            &mut aggregates,
            format!("{}_memory.csv", processing_model.to_string()),
        );
        results.push((*processing_model, aggregates));
    }
    plot_aggregate_data("memory_usage".to_string(), results);
}

fn aggregate_alert_delays(processing_models: &Vec<RequestProcessingModel>) {
    let mut results = vec![];
    for processing_model in processing_models {
        let mut aggregates: ResultVector = vec![];
        let dir_entries = get_relevant_files(processing_model, "ad");
        for dir_entry in dir_entries {
            let no_motor_groups =
                get_independent_variable(dir_entry.file_name().into_string().unwrap());
            let series = read_csv_to_series(dir_entry);
            append_aggregates(&mut aggregates, no_motor_groups, &series);
        }
        write_results_as_csv(
            &mut aggregates,
            format!("{}_delays.csv", processing_model.to_string()),
        );
        results.push((*processing_model, aggregates));
    }
    plot_aggregate_data("alert_delays".to_string(), results);
}

fn aggregate_alert_failures(processing_models: &Vec<RequestProcessingModel>) {
    let mut results = vec![];
    for processing_model in processing_models {
        let mut aggregates: ResultVector = vec![];
        let dir_entries = get_relevant_files(processing_model, "af");
        for dir_entry in dir_entries {
            let no_motor_groups =
                get_independent_variable(dir_entry.file_name().into_string().unwrap());
            let series = read_csv_to_series(dir_entry);
            append_aggregates(&mut aggregates, no_motor_groups, &series);
        }
        write_results_as_csv(
            &mut aggregates,
            format!("{}_failures.csv", processing_model.to_string()),
        );
        results.push((*processing_model, aggregates));
    }
    plot_aggregate_data("alert_failures".to_string(), results);
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

fn get_independent_variable(file_name: String) -> i32 {
    let motor_groups = file_name
        .split('_')
        .nth(2)
        .expect("Resource usage file should start with the number of motor groups")
        .parse::<i32>()
        .expect("nth number should be parsable to integer");
    motor_groups
}

fn append_aggregates(aggregates: &mut ResultVector, no_motor_groups: i32, series: &Series) {
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

fn write_results_as_csv(aggregates: &mut ResultVector, file_name: String) {
    aggregates.sort_by_key(|agg| agg.0);
    let result = aggregates
        .iter()
        .map(|(id, min, mean, max, std)| format!("{id},{min},{mean},{max},{std}"))
        .collect::<Vec<String>>()
        .join("\n");
    fs::write(file_name, format!("no,min,mean,max,std\n{result}"))
        .expect("Should be able to write processing model results");
}

fn get_data_frames(
    processing_model: &RequestProcessingModel,
    file_name_marker: &str,
) -> Vec<(String, DataFrame)> {
    let mut schema = Schema::new();
    schema.with_column("id".parse().unwrap(), DataType::Int64);
    schema.with_column("utime".parse().unwrap(), DataType::Int64);
    schema.with_column("stime".parse().unwrap(), DataType::Int64);
    schema.with_column("cutime".parse().unwrap(), DataType::Int64);
    schema.with_column("cstime".parse().unwrap(), DataType::Int64);
    schema.with_column("vmhwm".parse().unwrap(), DataType::Int64);
    schema.with_column("vmpeak".parse().unwrap(), DataType::Int64);

    let schema = Arc::new(schema);

    get_relevant_files(processing_model, file_name_marker)
        .iter()
        .map(|dir_entry| {
            let schema = Arc::clone(&schema);
            (
                dir_entry
                    .file_name()
                    .into_string()
                    .expect("Result file should have UTF-8 name"),
                CsvReader::from_path(dir_entry.path())
                    .map(move |csv_reader| {
                        csv_reader
                            .has_header(true)
                            .with_dtypes(Some(schema))
                            .finish()
                            .expect("Result file should be readable as csv")
                    })
                    .expect("Result file should be readable as data frame"),
            )
        })
        .collect::<Vec<(String, DataFrame)>>()
}

fn get_relevant_files(
    processing_model: &RequestProcessingModel,
    file_name_marker: &str,
) -> Vec<DirEntry> {
    read_dir(RAW_DATA_PATH)
        .expect("Raw data directory should exist and be readable")
        .filter_map(|dir_entry| dir_entry.ok())
        .filter_map(|dir_entry| {
            if let Ok(file_name) = dir_entry.file_name().into_string() {
                if file_name.contains(&processing_model.to_string())
                    && file_name.contains(file_name_marker)
                    && file_name.starts_with("1_")
                {
                    return Some(dir_entry);
                }
            }
            None
        })
        .collect()
}

fn plot_aggregate_data(
    data_name: String,
    processing_model_runs: Vec<(RequestProcessingModel, ResultVector)>,
) {
    let file_name = format!("figures/{data_name}.svg");
    let root_drawing_area = SVGBackend::new(&file_name, (1024, 768)).into_drawing_area();
    root_drawing_area.fill(&WHITE).unwrap();
    root_drawing_area
        .titled(&data_name, ("sans-serif", 40))
        .unwrap();
    let mut chart = ChartBuilder::on(&root_drawing_area)
        .margin(25)
        .set_left_and_bottom_label_area_size(20)
        .build_cartesian_2d(
            get_independent_range(&processing_model_runs).log_scale(),
            get_dependent_range(&processing_model_runs),
        )
        .unwrap();
    chart
        .configure_mesh()
        .x_desc(X_LABEL)
        .y_desc(data_name)
        .draw()
        .unwrap();
    for (processing_model, results_vector) in processing_model_runs {
        let style = match processing_model {
            RequestProcessingModel::ReactiveStreaming => RED,
            RequestProcessingModel::ClientServer => BLUE,
            RequestProcessingModel::SpringQL => GREEN,
            RequestProcessingModel::ObjectOriented => BLACK,
        };
        chart
            .draw_series(results_vector.iter().map(|single_run| {
                ErrorBar::new_vertical(
                    single_run.0,
                    single_run.2 - single_run.4,
                    single_run.2,
                    single_run.2 + single_run.4,
                    style.clone(),
                    10,
                )
            }))
            .unwrap()
            .label(processing_model.to_string())
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], style));
    }
    chart
        .configure_series_labels()
        .border_style(&BLACK)
        .draw()
        .unwrap();
}

fn get_independent_range(
    processing_model_runs: &[(RequestProcessingModel, ResultVector)],
) -> Range<i32> {
    let independent_averages = processing_model_runs
        .iter()
        .flat_map(|(_, results)| results)
        .map(|result| result.0);
    independent_averages
        .clone()
        .min()
        .expect("At least one measurement should be present")
        ..independent_averages
            .max()
            .expect("At least one measurement should be present")
}

fn get_dependent_range(
    processing_model_runs: &[(RequestProcessingModel, ResultVector)],
) -> Range<f64> {
    let independent_averages = processing_model_runs
        .iter()
        .flat_map(|(_, results)| results);
    // independent_averages
    //     .clone()
    //     .map(|result| result.2 - result.4)
    //     .reduce(f64::min)
    //     .expect("At least one measurement should be present")
    0f64..independent_averages
        .map(|result| result.2 + result.4)
        .reduce(f64::max)
        .expect("At least one measurement should be present")
}
