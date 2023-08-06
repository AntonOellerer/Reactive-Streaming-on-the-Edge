use std::cmp::Ordering;
use std::fs;
use std::fs::{read_dir, DirEntry};
use std::ops::Range;
use std::str::FromStr;
use std::sync::Arc;

use plotters::prelude::{
    ChartBuilder, ErrorBar, IntoDrawingArea, PathElement, SVGBackend, BLACK, BLUE, GREEN, RED,
    WHITE,
};
use polars::datatypes::DataType;
use polars::frame::DataFrame;
use polars::prelude::SerReader;
use polars::prelude::{ChunkVar, Series};
use polars::prelude::{CsvReader, Schema};

use data_transfer_objects::RequestProcessingModel;

const RAW_DATA_PATH: &str = "../bench_executor/";
const X_LABEL: &str = "Window Size";

type ResultVector = Vec<(Axes, f64, f64, f64, f64)>;
type ResultSet = (f64, f64, f64, f64);

struct ResultFrame<T> {
    independent_variable: usize,
    processing_model: RequestProcessingModel,
    data: T,
}

struct ResultDiagram<T> {
    independent_variable: usize,
    frames: Vec<ResultFrame<T>>,
}

struct ResultRow<T> {
    independent_variable: usize,
    results: Vec<ResultDiagram<T>>,
}

type ResultMatrix<T> = Vec<ResultRow<T>>;

#[derive(Eq, PartialEq, Copy, Clone)]
struct Axes {
    x_inner: usize,
    x_outer: usize,
    y_outer: usize,
}

fn main() {
    let axis_indices = get_axes_indices();

    let processing_models = vec![
        RequestProcessingModel::ObjectOriented,
        RequestProcessingModel::ReactiveStreaming,
    ];
    aggregate_processing_time(&axis_indices);
    // aggregate_memory_usage(&axis_indices, &processing_models);
    // aggregate_alert_delays(&axis_indices, &processing_models);
    // aggregate_alert_failures(&axis_indices, &processing_models);
    // aggregate_load_average(&axis_indices, &processing_models);
}

fn get_axes_indices() -> Axes {
    if let [x_inner, x_outer, y_outer] = &std::env::args()
        .skip(1)
        .take(4)
        .map(|token| token.parse().unwrap())
        .collect::<Vec<usize>>()[..]
    {
        return Axes {
            x_inner: *x_inner,
            x_outer: *x_outer,
            y_outer: *y_outer,
        };
    } else {
        panic!("Did not receive axes arguments");
    }
}

fn aggregate_processing_time(axis_indices: &Axes) {
    let mut aggregates: ResultMatrix<ResultSet> = vec![];
    let result_matrix = get_data_frames(axis_indices, "ru");
    for row in result_matrix {
        let mut aggregates_row = ResultRow {
            independent_variable: row.independent_variable,
            results: vec![],
        };
        for diagram in row.results {
            let mut aggregate_diagram = ResultDiagram {
                independent_variable: diagram.independent_variable,
                frames: vec![],
            };
            for frame in diagram.frames {
                let data_frame = frame.data;
                let total_time = &(&(&data_frame["utime"] + &data_frame["stime"])
                    + &data_frame["cutime"])
                    + &data_frame["cstime"];
                let aggregate_frame = ResultFrame {
                    independent_variable: frame.independent_variable,
                    processing_model: frame.processing_model,
                    data: get_aggregates(&total_time),
                };
                aggregate_diagram.frames.push(aggregate_frame);
            }
            aggregates_row.results.push(aggregate_diagram);
        }
        aggregates.push(aggregates_row);
    }
    plot_aggregate_data("processing_time".to_string(), aggregates);
}
//
// fn aggregate_memory_usage(axis_indices: &Axes, processing_models: &Vec<RequestProcessingModel>) {
//     let mut results = vec![];
//     for processing_model in processing_models {
//         let mut aggregates: ResultVector = vec![];
//         let data_frames = get_data_frames(processing_model, "ru");
//         for (file_name, data_frame) in data_frames {
//             let axes = get_axis_variables(axis_indices, file_name);
//             let vmhwm = &data_frame["vmhwm"];
//             append_aggregates(&mut aggregates, axes, vmhwm);
//         }
//         write_results_as_csv(
//             &mut aggregates,
//             format!("{}_memory.csv", processing_model.to_string()),
//         );
//         results.push((*processing_model, aggregates));
//     }
//     plot_aggregate_data("memory_usage".to_string(), results);
// }
//
// fn aggregate_alert_delays(axis_indices: &Axes, processing_models: &Vec<RequestProcessingModel>) {
//     let mut results = vec![];
//     for processing_model in processing_models {
//         let mut aggregates: ResultVector = vec![];
//         let dir_entries = get_relevant_files(processing_model, "ad");
//         for dir_entry in dir_entries {
//             let axes =
//                 get_axis_variables(axis_indices, dir_entry.file_name().into_string().unwrap());
//             let series = read_csv_to_series(dir_entry);
//             append_aggregates(&mut aggregates, axes, &series);
//         }
//         write_results_as_csv(
//             &mut aggregates,
//             format!("{}_delays.csv", processing_model.to_string()),
//         );
//         results.push((*processing_model, aggregates));
//     }
//     plot_aggregate_data("alert_delays".to_string(), results);
// }
//
// fn aggregate_alert_failures(axis_indices: &Axes, processing_models: &Vec<RequestProcessingModel>) {
//     let mut results = vec![];
//     for processing_model in processing_models {
//         let mut aggregates: ResultVector = vec![];
//         let dir_entries = get_relevant_files(processing_model, "af");
//         for dir_entry in dir_entries {
//             let axes =
//                 get_axis_variables(axis_indices, dir_entry.file_name().into_string().unwrap());
//             let series = read_csv_to_series(dir_entry);
//             append_aggregates(&mut aggregates, axes, &series);
//         }
//         write_results_as_csv(
//             &mut aggregates,
//             format!("{}_failures.csv", processing_model.to_string()),
//         );
//         results.push((*processing_model, aggregates));
//     }
//     plot_aggregate_data("alert_failures".to_string(), results);
// }
//
// fn aggregate_load_average(axis_indices: &Axes, processing_models: &Vec<RequestProcessingModel>) {
//     let mut results = vec![];
//     for processing_model in processing_models {
//         let mut aggregates: ResultVector = vec![];
//         let data_frames = get_data_frames(processing_model, "ru");
//         for (file_name, data_frame) in data_frames {
//             let axes = get_axis_variables(axis_indices, file_name);
//             let load_average = &data_frame["load_average"];
//             append_aggregates(&mut aggregates, axes, load_average);
//         }
//         write_results_as_csv(
//             &mut aggregates,
//             format!("{}_load_average.csv", processing_model.to_string()),
//         );
//         results.push((*processing_model, aggregates));
//     }
//     plot_aggregate_data("load_average".to_string(), results);
// }

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

fn get_axis_variables(axes: &Axes, file_name: &String) -> Axes {
    let independent_variables = get_independent_variables(&file_name);
    Axes {
        x_outer: independent_variables[axes.x_outer],
        y_outer: independent_variables[axes.y_outer],
        x_inner: independent_variables[axes.x_inner],
    }
}

fn get_request_processing_model(file_name: &String) -> RequestProcessingModel {
    let request_processing_model = file_name.split('_').nth(6).unwrap();
    RequestProcessingModel::from_str(request_processing_model).unwrap()
}

fn get_independent_variables(file_name: &str) -> Vec<usize> {
    file_name
        .split('_')
        .filter_map(|token| token.parse().ok())
        .collect::<Vec<usize>>()
}

fn get_aggregates(series: &Series) -> ResultSet {
    let std = match series.f64() {
        Ok(cast) => cast.std(1).unwrap_or(0.0),
        Err(_) => match series.i64() {
            Ok(cast) => cast.std(1).unwrap_or(0.0),
            Err(_) => 0.0,
        },
    };
    (
        series.min().unwrap_or(0.0),
        series.mean().unwrap_or(0.0),
        series.max().unwrap_or(0.0),
        std,
    )
}

fn append_aggregates(aggregates: &mut ResultVector, axes: Axes, series: &Series) {
    let std = match series.f64() {
        Ok(cast) => cast.std(1).unwrap_or(0.0),
        Err(_) => match series.i64() {
            Ok(cast) => cast.std(1).unwrap_or(0.0),
            Err(_) => 0.0,
        },
    };
    aggregates.push((
        axes,
        series.min().unwrap_or(0.0),
        series.mean().unwrap_or(0.0),
        series.max().unwrap_or(0.0),
        std,
    ));
}

fn write_results_as_csv(aggregates: &mut ResultVector, file_name: String) {
    let result = aggregates
        .iter()
        .map(|(axes, min, mean, max, std)| {
            format!(
                "{}, {}, {},{min},{mean},{max},{std}",
                axes.y_outer, axes.x_outer, axes.x_inner
            )
        })
        .collect::<Vec<String>>()
        .join("\n");
    fs::write(file_name, format!("no,min,mean,max,std\n{result}"))
        .expect("Should be able to write processing model results");
}

fn get_data_frames(axis_indices: &Axes, file_name_marker: &str) -> ResultMatrix<DataFrame> {
    let mut schema = Schema::new();
    schema.with_column("id".parse().unwrap(), DataType::Int64);
    schema.with_column("utime".parse().unwrap(), DataType::Int64);
    schema.with_column("stime".parse().unwrap(), DataType::Int64);
    schema.with_column("cutime".parse().unwrap(), DataType::Int64);
    schema.with_column("cstime".parse().unwrap(), DataType::Int64);
    schema.with_column("vmhwm".parse().unwrap(), DataType::Int64);
    schema.with_column("vmpeak".parse().unwrap(), DataType::Int64);
    schema.with_column("load_average".parse().unwrap(), DataType::Float32);

    let schema = Arc::new(schema);

    let result_set = get_relevant_files(file_name_marker)
        .iter()
        .map(|dir_entry| {
            let schema = Arc::clone(&schema);
            let file_name = dir_entry
                .file_name()
                .into_string()
                .expect("Result file should have UTF-8 name");
            (
                get_axis_variables(axis_indices, &file_name),
                get_request_processing_model(&file_name),
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
        .collect::<Vec<(Axes, RequestProcessingModel, DataFrame)>>();
    frames_to_matrix(result_set)
}

fn frames_to_matrix(
    mut result_set: Vec<(Axes, RequestProcessingModel, DataFrame)>,
) -> ResultMatrix<DataFrame> {
    result_set.sort_by(|(axes_1, _, _), (axes_2, _, _)| {
        if axes_1.y_outer.cmp(&axes_2.y_outer) == Ordering::Equal {
            if axes_1.x_outer.cmp(&axes_2.x_outer) == Ordering::Equal {
                axes_1.x_inner.cmp(&axes_2.x_inner)
            } else {
                axes_1.x_outer.cmp(&axes_2.x_outer)
            }
        } else {
            axes_1.y_outer.cmp(&axes_2.y_outer)
        }
    });
    let mut result_matrix: ResultMatrix<DataFrame> = vec![];
    let mut last_axes = result_set[0].0;
    for (axes, request_processing_model, data_frame) in result_set {
        let frame = ResultFrame {
            independent_variable: axes.x_inner,
            processing_model: request_processing_model,
            data: data_frame,
        };
        if result_matrix.is_empty() || axes.y_outer.cmp(&last_axes.y_outer) != Ordering::Equal {
            let diagram = ResultDiagram {
                independent_variable: axes.x_outer,
                frames: vec![frame],
            };
            let new_row = ResultRow {
                independent_variable: axes.y_outer,
                results: vec![diagram],
            };
            result_matrix.push(new_row);
        } else if axes.x_outer.cmp(&last_axes.x_outer) != Ordering::Equal {
            let diagram = ResultDiagram {
                independent_variable: axes.x_outer,
                frames: vec![frame],
            };
            let test = result_matrix.iter_mut().last().unwrap();
            let row = &mut test.results;
            row.push(diagram);
        } else {
            let row = &mut result_matrix.iter_mut().last().unwrap().results;
            let diagram = &mut row.iter_mut().last().unwrap().frames;
            diagram.push(frame);
        }
        last_axes = axes;
    }
    result_matrix
}

fn get_relevant_files(file_name_marker: &str) -> Vec<DirEntry> {
    read_dir(RAW_DATA_PATH)
        .expect("Raw data directory should exist and be readable")
        .filter_map(|dir_entry| dir_entry.ok())
        .filter_map(|dir_entry| {
            if let Ok(file_name) = dir_entry.file_name().into_string() {
                if file_name.contains(file_name_marker) && file_name.ends_with(".csv") {
                    return Some(dir_entry);
                }
            }
            None
        })
        .collect()
}

fn plot_aggregate_data(data_name: String, aggregate_matrix: ResultMatrix<ResultSet>) {
    let file_name = format!("figures/{data_name}.svg");
    let root_drawing_area = SVGBackend::new(&file_name, (3 * 1024, 3 * 768)).into_drawing_area();
    root_drawing_area.fill(&WHITE).unwrap();
    root_drawing_area
        .titled(&data_name, ("sans-serif", 40))
        .unwrap();
    let columns = aggregate_matrix.first().unwrap().results.len();
    let panels = root_drawing_area.split_evenly((aggregate_matrix.len(), columns));
    for (y_index, row) in aggregate_matrix.iter().enumerate() {
        for (x_index, diagram) in row.results.iter().enumerate() {
            if x_index > 2 {
                continue;
            }
            let mut chart = ChartBuilder::on(&panels[y_index * columns + x_index])
                .margin(25)
                .set_left_and_bottom_label_area_size(20)
                .build_cartesian_2d(get_independent_range(diagram), get_dependent_range(diagram))
                .unwrap();
            chart
                .configure_mesh()
                .x_desc(X_LABEL)
                .y_desc(data_name.clone())
                .draw()
                .unwrap();
            for frame in diagram.frames.iter() {
                let style = match frame.processing_model {
                    RequestProcessingModel::ReactiveStreaming => RED,
                    RequestProcessingModel::ClientServer => BLUE,
                    RequestProcessingModel::SpringQL => GREEN,
                    RequestProcessingModel::ObjectOriented => BLACK,
                };
                chart
                    .draw_series(std::iter::once(ErrorBar::new_vertical(
                        frame.independent_variable,
                        frame.data.1 - frame.data.3,
                        frame.data.1,
                        frame.data.1 + frame.data.3,
                        style,
                        10,
                    )))
                    .unwrap()
                    .label(frame.processing_model.to_string())
                    .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], style));
            }
            chart
                .configure_series_labels()
                .border_style(BLACK)
                .draw()
                .unwrap();
        }
    }
}

fn get_independent_range(diagram: &ResultDiagram<ResultSet>) -> Range<usize> {
    let independent_averages = diagram
        .frames
        .iter()
        .map(|frame| frame.independent_variable);
    independent_averages
        .clone()
        .min()
        .expect("At least one measurement should be present")
        ..independent_averages
            .max()
            .expect("At least one measurement should be present")
}

fn get_dependent_range(diagram: &ResultDiagram<ResultSet>) -> Range<f64> {
    let independent_averages = diagram.frames.iter().map(|frame| frame.data);
    0f64..independent_averages
        .map(|result| result.2 + result.3)
        .reduce(f64::max)
        .expect("At least one measurement should be present")
}
