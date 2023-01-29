use bollard::models::{Network, Service};
use bollard::network::InspectNetworkOptions;
use bollard::service::{InspectServiceOptions, UpdateServiceOptions};
use bollard::{ClientVersion, Docker};
use data_transfer_objects::RequestProcessingModel;
use futures::FutureExt;
use serde::Deserialize;
use std::fs::File;
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::time::Duration;
use std::{fs, thread};

#[derive(Deserialize)]
struct Config {
    repetitions: u32,
    motor_groups_tcp: Vec<u16>,
    durations: Vec<u64>,
    request_processing_models: Vec<RequestProcessingModel>,
    window_size_seconds: Vec<u64>,
    sampling_interval_ms: Vec<u32>,
    thread_pool_sizes: Vec<usize>,
}

#[cfg(debug_assertions)]
const CONFIG_PATH: &str = "resources/config-debug.toml";
#[cfg(not(debug_assertions))]
const CONFIG_PATH: &str = "/etc/config-production.toml";

/// expects a running swarm w/ the stack deployed
#[tokio::main]
async fn main() {
    let config: Config =
        toml::from_str(&fs::read_to_string(CONFIG_PATH).expect("Could not read config file"))
            .expect("Could not parse config file");
    let docker = Docker::connect_with_unix(
        "/run/user/1000/docker.sock",
        120,
        &ClientVersion {
            major_version: 1,
            minor_version: 41,
        },
    )
    .unwrap();
    for no_motor_groups in &config.motor_groups_tcp {
        scale_service(*no_motor_groups, &docker).await;
        // for duration in &config.durations {
        //     for window_size_seconds in &config.window_size_seconds {
        //         for sampling_interval_ms in &config.sampling_interval_ms {
        //             for thread_pool_size in &config.thread_pool_sizes {
        //                 for request_processing_model in &config.request_processing_models {
        //                     let file_name = format!("{no_motor_groups}_{duration}_{window_size_seconds}_{sampling_interval_ms}_{thread_pool_size}_{}.csv", request_processing_model.to_string());
        //                     let mut file = OpenOptions::new()
        //                         .write(true)
        //                         .append(true)
        //                         .open(file_name)
        //                         .unwrap();
        //                     write!(file, "id,utime,ctime,cutime,cstime,vmhwm,vmpeak").unwrap();
        //                     for i in 0..config.repetitions {
        //                         info!("{i} {no_motor_groups} {duration} {window_size_seconds} {sampling_interval_ms} {thread_pool_size} {}", request_processing_model.to_string());
        //                         let results = execute_test_run(
        //                             *no_motor_groups,
        //                             *duration,
        //                             *window_size_seconds,
        //                             *sampling_interval_ms,
        //                             *thread_pool_size,
        //                             *request_processing_model,
        //                             &docker,
        //                         );
        //                         write!(file, "{results}").unwrap();
        //                     }
        //                 }
        //             }
        //         }
        //     }
        // }
    }
}

async fn scale_service(no_motor_groups: u16, docker: &Docker) {
    let mut sensor_socket_addresses =
        File::create("../test_driver/sensor_socket_addresses.txt").unwrap();
    let execution_chain = docker
        .inspect_service("bench_system_sensor", None::<InspectServiceOptions>)
        .then(|current| {
            let mut current = current.unwrap();
            let options = UpdateServiceOptions {
                version: current.version.as_mut().unwrap().index.unwrap(),
                ..Default::default()
            };
            get_spec(no_motor_groups, &mut current);
            docker
                .update_service("bench_system_sensor", current.spec.unwrap(), options, None)
                .then(|d| async move {
                    eprintln!("{d:#?}");
                    let mut virtual_ips = Vec::new();
                    while virtual_ips.len() < (no_motor_groups * 4).into() {
                        thread::sleep(Duration::from_secs(5));
                        let service_result = docker
                            .inspect_network(
                                "bench_system_default",
                                None::<InspectNetworkOptions<String>>,
                            )
                            .await
                            .unwrap();
                        virtual_ips = get_ips(service_result);
                    }
                    virtual_ips
                })
        });
    let ips: Vec<SocketAddr> = execution_chain.await;
    ips.iter()
        .for_each(|ip| writeln!(sensor_socket_addresses, "{ip}").unwrap());
}

fn get_ips(network: Network) -> Vec<SocketAddr> {
    network
        .containers
        .unwrap()
        .iter()
        .filter(|(_, container)| {
            container
                .name
                .as_ref()
                .unwrap()
                .starts_with("bench_system_sensor")
        })
        .map(|(_, container)| container.ipv4_address.as_ref().unwrap().clone())
        .map(|ipv4_address| {
            let addr = ipv4_address.split('/').next().unwrap();
            SocketAddr::new(IpAddr::from_str(addr).unwrap(), 11000)
        })
        .collect()
}

fn get_spec(no_motor_groups: u16, current: &mut Service) {
    current
        .spec
        .as_mut()
        .unwrap()
        .mode
        .as_mut()
        .unwrap()
        .replicated
        .as_mut()
        .unwrap()
        .replicas = Some((no_motor_groups * 4).into());
}

fn execute_test_run(
    no_motor_groups: u16,
    duration: u64,
    window_size_seconds: u64,
    sampling_interval_ms: u32,
    thread_pool_size: usize,
    request_processing_model: RequestProcessingModel,
) -> String {
    // let containers = docker.containers();
    // let mut opt_vec = vec![];
    // // let mut creates = vec![];
    // for i in 0..no_motor_groups {
    //     for j in 0..4 {
    //         let opts = ContainerCreateOpts::builder()
    //             .image("aom/sensor:0.1.0")
    //             .portmappings(vec![PortMapping {
    //                 container_port: Some(11000),
    //                 host_ip: None,
    //                 host_port: Some(i * 4 + j),
    //                 protocol: None,
    //                 range: None,
    //             }])
    //             .build();
    //         opt_vec.push(opts);
    //     }
    // }
    // let creates = async {
    //     let mut creates = vec![];
    //     for opt in opt_vec.iter() {
    //         creates.push(containers.create(opt));
    //     }
    //     creates
    // };
    // executor::block_on(creates);
    let mut command = Command::new("cargo");
    command
        .current_dir("../test_driver")
        .arg("run")
        .arg("--release")
        .arg("--")
        .arg("--motor-groups-tcp")
        .arg(no_motor_groups.to_string())
        .arg("--duration")
        .arg(duration.to_string())
        .arg("--window-size-seconds")
        .arg(window_size_seconds.to_string())
        .arg("--sampling-interval-ms")
        .arg(sampling_interval_ms.to_string())
        .arg("--thread-pool-size")
        .arg(thread_pool_size.to_string())
        .arg(request_processing_model.to_string())
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .output()
        .expect("Failure when trying to run sensor program");

    fs::read_to_string("../test_driver/motor_monitor_results.csv").unwrap()
}
