use data_transfer_objects::RequestProcessingModel;
use plotters::prelude::{
    Boxplot, ChartBuilder, Circle, IntoDrawingArea, IntoLogRange, PathElement, Quartiles,
    SVGBackend, BLACK, BLUE, GREEN, RED, WHITE,
};
use plotters::series::LineSeries;
use plotters::style::TRANSPARENT;
use polars::datatypes::DataType;
use polars::export::ahash::{HashMap, HashMapExt};
use polars::frame::DataFrame;
use polars::prelude::Series;
use polars::prelude::{ChunkVar, SerReader};
use polars::prelude::{CsvReader, Schema};
use statrs::distribution::{ContinuousCDF, StudentsT};
use std::cmp::Ordering;
use std::env::Args;
use std::fs;
use std::fs::{read_dir, DirEntry, OpenOptions};
use std::io::Write;
use std::ops::Range;
use std::str::FromStr;
use std::sync::Arc;

const X_LABEL: &str = "Window Size (in ms)";

const SIGNIFICANCE_LEVEL: f64 = 0.05;

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

#[derive(PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Debug)]
enum System {
    Local,
    Dsg,
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
struct Axes {
    x_inner: usize,
    x_outer: System,
    y_outer: usize,
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
        y_outer: args
            .next()
            .map(|token| token.parse::<usize>().unwrap())
            .unwrap(),
        x_outer: System::Local,
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
            for frame in diagram.frames.clone() {
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
            diagram
                .frames
                .iter()
                .fold(HashMap::new(), |mut acc, frame| {
                    let entry = acc
                        .entry(frame.independent_variable)
                        .or_insert((None, None));
                    if frame.processing_model == RequestProcessingModel::ReactiveStreaming {
                        entry.0 = Some(&frame.data)
                    } else {
                        entry.1 = Some(&frame.data)
                    }
                    acc
                })
                .iter()
                .filter(|(_, (rx_frame, oo_frame))| rx_frame.is_some() && oo_frame.is_some())
                .for_each(|(key, (rx_frame, oo_frame))| {
                    let rx_series = extract_data(rx_frame.unwrap());
                    let oo_series = extract_data(oo_frame.unwrap());
                    let p_value = t_test(&rx_series, &oo_series); //rx > oo
                    if p_value > SIGNIFICANCE_LEVEL {
                        let p_value_c = t_test(&oo_series, &rx_series); // oo > rx
                        if p_value_c > SIGNIFICANCE_LEVEL {
                            println!(
                                "Equal performance: {data_name} {} {} {key} {p_value}",
                                row.independent_variable, diagram.independent_variable
                            )
                        } else {
                            println!(
                                "Declarative better performance: {data_name} {} {} {key} {p_value}",
                                row.independent_variable, diagram.independent_variable
                            )
                        }
                    }
                });
            aggregates_row.results.push(aggregate_diagram);
        }
        aggregates.push(aggregates_row);
    }
    plot_aggregate_data(data_name, aggregates);
}

fn t_test(series1: &Series, series2: &Series) -> f64 {
    let min_length = std::cmp::min(series1.len(), series2.len());
    if min_length < 2 {
        return 0f64;
    }
    let difference = series1.head(Some(min_length)) - series2.head(Some(min_length));
    let diff_mean = difference.mean().unwrap();
    let diff_std = match difference.i64() {
        Ok(i) => i.std(1).unwrap(),
        Err(_) => match difference.f32() {
            Ok(i) => i.std(1).unwrap() as f64,
            Err(_) => difference.f64().unwrap().std(1).unwrap(),
        },
    };
    let sample_size = difference.len() as f64;
    // println!("diff_mean: {diff_mean}, diff_std: {diff_std}, sample_size: {sample_size}");
    let t = diff_mean / (diff_std / sample_size.sqrt());
    let degrees_of_freedom = if sample_size <= 1f64 {
        1f64
    } else {
        sample_size - 1f64
    };
    let t_dist = StudentsT::new(0.0, 1.0, degrees_of_freedom).unwrap();
    // println!("t: {t} dof: {degrees_of_freedom}");
    1_f64 - t_dist.cdf(t)
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
    let mut lengths: ResultMatrix<usize> = vec![];
    let result_matrix = get_series(axis_indices, file_name_marker);
    for row in result_matrix {
        let mut aggregates_row = ResultRow {
            independent_variable: row.independent_variable,
            results: vec![],
        };
        let mut lengths_row = ResultRow {
            independent_variable: row.independent_variable,
            results: vec![],
        };
        for diagram in row.results {
            let mut aggregate_diagram = ResultDiagram {
                independent_variable: diagram.independent_variable,
                frames: vec![],
            };
            let mut length_diagram = ResultDiagram {
                independent_variable: diagram.independent_variable,
                frames: vec![],
            };
            for frame in diagram.frames.clone() {
                let quartiles = get_aggregates(&frame.data);
                save_as_csv(
                    data_name,
                    row.independent_variable,
                    diagram.independent_variable,
                    frame.independent_variable,
                    frame.processing_model,
                    &quartiles,
                );
                let aggregate_frame = ResultFrame {
                    independent_variable: frame.independent_variable,
                    processing_model: frame.processing_model,
                    data: quartiles,
                };
                aggregate_diagram.frames.push(aggregate_frame);
                let length_frame = ResultFrame {
                    independent_variable: frame.independent_variable,
                    processing_model: frame.processing_model,
                    data: frame.data.len(),
                };
                length_diagram.frames.push(length_frame);
            }
            diagram
                .frames
                .iter()
                .fold(HashMap::new(), |mut acc, frame| {
                    let entry = acc
                        .entry(frame.independent_variable)
                        .or_insert((None, None));
                    if frame.processing_model == RequestProcessingModel::ReactiveStreaming {
                        entry.0 = Some(&frame.data)
                    } else {
                        entry.1 = Some(&frame.data)
                    }
                    acc
                })
                .iter()
                .for_each(|(key, (rx_series, oo_series))| {
                    let p_value = t_test(rx_series.unwrap(), oo_series.unwrap()); // rx > oo
                    if p_value > SIGNIFICANCE_LEVEL {
                        let p_value_c = t_test(oo_series.unwrap(), rx_series.unwrap()); // oo > rx
                        if p_value_c > SIGNIFICANCE_LEVEL {
                            println!(
                                "Equal performance: {data_name} {} {} {key} {p_value}",
                                row.independent_variable, diagram.independent_variable
                            )
                        } else {
                            println!(
                                "Declarative better performance: {data_name} {} {} {key} {p_value}",
                                row.independent_variable, diagram.independent_variable
                            )
                        }
                    }
                });
            aggregates_row.results.push(aggregate_diagram);
            lengths_row.results.push(length_diagram);
        }
        aggregates.push(aggregates_row);
        lengths.push(lengths_row);
    }
    plot_aggregate_data(data_name, aggregates);
    plot_simple_data("number_of_alerts", lengths);
}

fn get_axis_variables(axes: &Axes, file_name: &str, system: System) -> Axes {
    let independent_variables = get_independent_variables(file_name);
    Axes {
        x_inner: independent_variables[axes.x_inner],
        x_outer: system,
        y_outer: independent_variables[axes.y_outer],
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
            let system = if dir_entry.path().parent().unwrap().ends_with("dsg_data") {
                System::Dsg
            } else {
                System::Local
            };
            (
                get_axis_variables(axis_indices, &file_name, system),
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
    read_dir("data/dsg_data")
        .unwrap()
        .chain(read_dir("data/local_data").unwrap())
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
        if result_matrix.is_empty() || (axes.y_outer.cmp(&last_axes.y_outer) != Ordering::Equal) {
            let diagram = ResultDiagram {
                independent_variable: if axes.x_outer == System::Dsg { 1 } else { 0 },
                frames: vec![frame],
            };
            let new_row = ResultRow {
                independent_variable: axes.y_outer,
                results: vec![diagram],
            };
            result_matrix.push(new_row);
        } else if axes.x_outer != last_axes.x_outer {
            let diagram = ResultDiagram {
                independent_variable: if axes.x_outer == System::Dsg { 1 } else { 0 },
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
            let system = if dir_entry.path().parent().unwrap().ends_with("dsg_data") {
                System::Dsg
            } else {
                System::Local
            };
            (
                get_axis_variables(axis_indices, &file_name, system),
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
    for (y_index, row) in aggregate_matrix.iter().enumerate() {
        for (x_index, diagram) in row.results.iter().enumerate() {
            let file_name = format!(
                "figures/{data_name}/{}_{}.svg",
                row.independent_variable, diagram.independent_variable
            );
            let root_drawing_area = SVGBackend::new(&file_name, (512, 512)).into_drawing_area();
            root_drawing_area.fill(&WHITE).unwrap();
            let dependent_range = get_dependent_range(diagram);
            let range_diff = dependent_range.end - dependent_range.start;
            let mut chart = ChartBuilder::on(&root_drawing_area)
                .margin(25)
                .x_label_area_size(35)
                .y_label_area_size(std::cmp::max(35, 15 * dependent_range.end.log10() as i32))
                .build_cartesian_2d(get_independent_range(diagram).log_scale(), dependent_range)
                .unwrap();
            let mut mesh = chart.configure_mesh();
            if range_diff >= 10f32 {
                mesh.y_label_formatter(&|y| format!("{y:.0}"));
            }
            mesh.x_desc(X_LABEL)
                .y_desc(get_y_desc(data_name))
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

fn get_y_desc(data_name: &str) -> String {
    match data_name {
        "alert_delays" => "Alert Delays (ms)".to_owned(),
        "load_average" => "Load Average (tasks per minute)".to_owned(),
        "memory_usage" => "Memory Usage (bytes)".to_owned(),
        "processing_time" => "Processing Time (ms)".to_owned(),
        _ => data_name.to_owned(),
    }
}

fn get_title(data_name: &str) -> String {
    match data_name {
        "alert_delays" => "Alert Delays".to_owned(),
        "load_average" => "Load Average".to_owned(),
        "memory_usage" => "Memory Usage".to_owned(),
        "processing_time" => "Processing Time".to_owned(),
        _ => data_name.to_owned(),
    }
}

fn plot_simple_data(data_name: &str, aggregate_matrix: ResultMatrix<usize>) {
    let rows = aggregate_matrix.len();
    let columns = aggregate_matrix.first().unwrap().results.len();
    let file_name = format!("figures/{data_name}.svg");
    let root_drawing_area =
        SVGBackend::new(&file_name, ((columns * 512) as u32, (rows * 512) as u32))
            .into_drawing_area();
    root_drawing_area.fill(&WHITE).unwrap();
    root_drawing_area
        .titled(&get_title(data_name), ("sans-serif", 40))
        .unwrap();
    let panels = root_drawing_area.split_evenly((rows, columns));
    for (y_index, row) in aggregate_matrix.iter().enumerate() {
        for (x_index, diagram) in row.results.iter().enumerate() {
            let mut chart = ChartBuilder::on(&panels[y_index * columns + x_index])
                .margin(25)
                .set_left_and_bottom_label_area_size(20)
                .build_cartesian_2d(
                    get_independent_range(diagram).log_scale(),
                    0f32..diagram.frames.iter().map(|d| d.data).max().unwrap_or(0) as f32,
                )
                .unwrap();
            chart
                .configure_mesh()
                .x_desc(X_LABEL)
                .y_desc(get_y_desc(data_name))
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
                    .draw(&Circle::new(
                        (frame.independent_variable as i32, frame.data as f32),
                        2,
                        style,
                    ))
                    .unwrap();
            }
        }
    }
}
fn get_independent_range<T>(diagram: &ResultDiagram<T>) -> Range<i32> {
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
