use data_transfer_objects::RequestProcessingModel;
use plotters::prelude::{
    Boxplot, ChartBuilder, IntoDrawingArea, IntoLogRange, Quartiles, SVGBackend, BLACK, BLUE,
    GREEN, RED, WHITE,
};
use polars::datatypes::DataType;
use polars::frame::DataFrame;
use polars::prelude::SerReader;
use polars::prelude::Series;
use polars::prelude::{CsvReader, Schema};
use std::cmp::Ordering;
use std::env::Args;
use std::fs;
use std::fs::{read_dir, DirEntry, OpenOptions};
use std::io::Write;
use std::ops::Range;
use std::str::FromStr;
use std::sync::Arc;

const RAW_DATA_PATH: &str = "../bench_executor/";
const X_LABEL: &str = "Window Size";

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
struct ResultFrame<T> {
    independent_variable: usize,
    processing_model: RequestProcessingModel,
    data: T,
}

#[derive(Eq, PartialEq, Clone, Debug)]
struct ResultDiagram<T> {
    independent_variable: usize,
    frames: Vec<ResultFrame<T>>,
}

#[derive(Eq, PartialEq, Clone, Debug)]
struct ResultRow<T> {
    independent_variable: usize,
    results: Vec<ResultDiagram<T>>,
}

type ResultMatrix<T> = Vec<ResultRow<T>>;

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
struct Axes {
    x_inner: usize,
    x_outer: Option<usize>,
    y_outer: Option<usize>,
}

fn main() {
    let axis_indices = get_axes_indices(&mut std::env::args());
    aggregate_data("processing_time", &axis_indices, |data_frame| {
        &(&(&data_frame["utime"] + &data_frame["stime"]) + &data_frame["cutime"])
            + &data_frame["cstime"]
    });
    aggregate_data("memory_usage", &axis_indices, |data_frame| {
        data_frame["vmhwm"].clone()
    });
    aggregate_data("load_average", &axis_indices, |data_frame| {
        data_frame["load_average"].clone()
    });
    aggregate_series("ad", "alert_delays", &axis_indices);
}

fn get_axes_indices(args: &mut Args) -> Axes {
    Axes {
        x_inner: args
            .nth(1)
            .map(|token| token.parse::<usize>().unwrap())
            .unwrap(),
        x_outer: args.next().and_then(|token| token.parse::<usize>().ok()),
        y_outer: args.next().and_then(|token| token.parse::<usize>().ok()),
    }
}

fn aggregate_data(data_name: &str, axis_indices: &Axes, extract_data: fn(&DataFrame) -> Series) {
    let mut aggregates: ResultMatrix<Quartiles> = vec![];
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
                let data_series = extract_data(&data_frame);
                let aggregate = get_aggregates(&data_series);
                save_as_csv(
                    data_name,
                    row.independent_variable,
                    diagram.independent_variable,
                    frame.independent_variable,
                    frame.processing_model,
                    &aggregate,
                );
                let aggregate_frame = ResultFrame {
                    independent_variable: frame.independent_variable,
                    processing_model: frame.processing_model,
                    data: aggregate,
                };
                aggregate_diagram.frames.push(aggregate_frame);
            }
            aggregates_row.results.push(aggregate_diagram);
        }
        aggregates.push(aggregates_row);
    }
    plot_aggregate_data(data_name, aggregates);
}

fn save_as_csv(
    data_name: &str,
    y_outer: usize,
    x_outer: usize,
    x_inner: usize,
    processing_model: RequestProcessingModel,
    quartiles: &Quartiles,
) {
    let [lower_fence, lower_quartile, median, upper_quartile, upper_fence] = quartiles.values();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!(
            "{data_name}_{y_outer}_{x_outer}_{processing_model:?}.csv"
        ))
        .unwrap();
    if file.metadata().unwrap().len() == 0 {
        writeln!(
            file,
            "independent_var, lower_fence, lower_quartile, median, upper_quartile, upper_fence"
        )
        .unwrap();
    }
    writeln!(
        file,
        "{x_inner}, {lower_fence}, {lower_quartile}, {median}, {upper_quartile}, {upper_fence}"
    )
    .unwrap();
}

fn aggregate_series(file_name_marker: &str, data_name: &str, axis_indices: &Axes) {
    let mut aggregates: ResultMatrix<Quartiles> = vec![];
    let result_matrix = get_series(axis_indices, file_name_marker);
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
                let aggregate_frame = ResultFrame {
                    independent_variable: frame.independent_variable,
                    processing_model: frame.processing_model,
                    data: get_aggregates(&frame.data),
                };
                aggregate_diagram.frames.push(aggregate_frame);
            }
            aggregates_row.results.push(aggregate_diagram);
        }
        aggregates.push(aggregates_row);
    }
    plot_aggregate_data(data_name, aggregates);
}

fn get_axis_variables(axes: &Axes, file_name: &str) -> Axes {
    let independent_variables = get_independent_variables(file_name);
    Axes {
        x_inner: independent_variables[axes.x_inner],
        x_outer: axes.x_outer.map(|idx| independent_variables[idx]),
        y_outer: axes.y_outer.map(|idx| independent_variables[idx]),
    }
}

fn get_request_processing_model(file_name: &str) -> RequestProcessingModel {
    let request_processing_model = file_name.split('_').nth(6).unwrap();
    RequestProcessingModel::from_str(request_processing_model).unwrap()
}

fn get_independent_variables(file_name: &str) -> Vec<usize> {
    file_name
        .split('_')
        .filter_map(|token| token.parse().ok())
        .collect::<Vec<usize>>()
}

fn get_aggregates(series: &Series) -> Quartiles {
    if series.is_empty() {
        Quartiles::new(&[0])
    } else {
        match series.f64() {
            Ok(chunked) => Quartiles::new(&chunked.into_no_null_iter().collect::<Vec<f64>>()),
            Err(_) => match series.f32() {
                Ok(chunked) => Quartiles::new(&chunked.into_no_null_iter().collect::<Vec<f32>>()),
                Err(_) => Quartiles::new(
                    &series
                        .i64()
                        .unwrap()
                        .into_no_null_iter()
                        .map(|i| i as i32)
                        .collect::<Vec<i32>>(),
                ),
            },
        }
    }
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
    data_to_matrix(result_set)
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

fn data_to_matrix<T>(mut result_set: Vec<(Axes, RequestProcessingModel, T)>) -> ResultMatrix<T> {
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
    let mut result_matrix: ResultMatrix<T> = vec![];
    let mut last_axes = result_set[0].0;
    for (axes, request_processing_model, data_frame) in result_set {
        let frame = ResultFrame {
            independent_variable: axes.x_inner,
            processing_model: request_processing_model,
            data: data_frame,
        };
        if result_matrix.is_empty()
            || (axes.y_outer.is_some() && axes.y_outer.cmp(&last_axes.y_outer) != Ordering::Equal)
        {
            let diagram = ResultDiagram {
                independent_variable: axes.x_outer.unwrap_or(0),
                frames: vec![frame],
            };
            let new_row = ResultRow {
                independent_variable: axes.y_outer.unwrap_or(0),
                results: vec![diagram],
            };
            result_matrix.push(new_row);
        } else if axes.x_outer.is_some() && axes.x_outer.cmp(&last_axes.x_outer) != Ordering::Equal
        {
            let diagram = ResultDiagram {
                independent_variable: axes.x_outer.unwrap_or(0),
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

fn get_series(axis_indices: &Axes, file_name_marker: &str) -> ResultMatrix<Series> {
    let result_set = get_relevant_files(file_name_marker)
        .iter()
        .map(|dir_entry| {
            let file_name = dir_entry
                .file_name()
                .into_string()
                .expect("Result file should have UTF-8 name");
            (
                get_axis_variables(axis_indices, &file_name),
                get_request_processing_model(&file_name),
                read_csv_to_series(dir_entry),
            )
        })
        .collect::<Vec<(Axes, RequestProcessingModel, Series)>>();
    data_to_matrix(result_set)
}

fn read_csv_to_series(dir_entry: &DirEntry) -> Series {
    let series: Series = fs::read_to_string(dir_entry.path())
        .expect("Series file should be readable to string")
        .split(',')
        .filter(|token| !token.is_empty())
        .map(f64::from_str)
        .map(Result::unwrap)
        .collect();
    series
}

fn plot_aggregate_data(data_name: &str, aggregate_matrix: ResultMatrix<Quartiles>) {
    let rows = aggregate_matrix.len();
    let columns = aggregate_matrix.first().unwrap().results.len();
    let file_name = format!("figures/{data_name}.svg");
    let root_drawing_area =
        SVGBackend::new(&file_name, ((columns * 512) as u32, (rows * 512) as u32))
            .into_drawing_area();
    root_drawing_area.fill(&WHITE).unwrap();
    root_drawing_area
        .titled(data_name, ("sans-serif", 40))
        .unwrap();
    let panels = root_drawing_area.split_evenly((rows, columns));
    for (y_index, row) in aggregate_matrix.iter().enumerate() {
        for (x_index, diagram) in row.results.iter().enumerate() {
            let mut chart = ChartBuilder::on(&panels[y_index * columns + x_index])
                .margin(25)
                .set_left_and_bottom_label_area_size(20)
                .build_cartesian_2d(
                    get_independent_range(diagram).log_scale(),
                    get_dependent_range(diagram),
                )
                .unwrap();
            chart
                .configure_mesh()
                .x_desc(X_LABEL)
                .y_desc(data_name)
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
                    .plotting_area()
                    .draw(
                        &Boxplot::new_vertical(frame.independent_variable as i32, &frame.data)
                            .style(style),
                    )
                    .unwrap();
            }
        }
    }
}

fn get_independent_range(diagram: &ResultDiagram<Quartiles>) -> Range<i32> {
    let independent_values = diagram
        .frames
        .iter()
        .map(|frame| frame.independent_variable as i32);
    0i32..independent_values
        .max()
        .expect("At least one measurement should be present")
}

fn get_dependent_range(diagram: &ResultDiagram<Quartiles>) -> Range<f32> {
    let dependent_values = diagram.frames.iter().map(|frame| &frame.data);
    0f32..dependent_values
        .map(|result| result.values()[4])
        .reduce(f32::max)
        .expect("At least one measurement should be present")
}
