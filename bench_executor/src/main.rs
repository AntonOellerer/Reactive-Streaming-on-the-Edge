extern crate core;

use bollard::models::{Network, Service};
use bollard::network::InspectNetworkOptions;
use bollard::service::{InspectServiceOptions, UpdateServiceOptions};
use bollard::{ClientVersion, Docker};
use data_transfer_objects::{NetworkConfig, RequestProcessingModel};
use futures::FutureExt;
use log::{debug, info, warn};
use serde::Deserialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::net::IpAddr;
use std::path::Path;
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

trait RAIIConfig {
    fn new(
        cloud_socket_address: IpAddr,
        motor_monitor_socket_address: IpAddr,
        sensor_addresses: Vec<IpAddr>,
    ) -> Self;
    fn update_sensor_addresses(&mut self, sensor_addresses: Vec<IpAddr>);
    fn persist(&self);
}

impl RAIIConfig for NetworkConfig {
    fn new(
        cloud_server_socket_address: IpAddr,
        motor_monitor_socket_address: IpAddr,
        sensor_addresses: Vec<IpAddr>,
    ) -> NetworkConfig {
        let network_config = NetworkConfig {
            cloud_server_address: cloud_server_socket_address,
            motor_monitor_address: motor_monitor_socket_address,
            sensor_addresses,
        };
        network_config.persist();
        network_config
    }

    fn update_sensor_addresses(&mut self, sensor_addresses: Vec<IpAddr>) {
        self.sensor_addresses = sensor_addresses;
        self.persist();
    }

    fn persist(&self) {
        let config =
            toml::to_string(&self).expect("Could not create toml string from network config");
        fs::write(Path::new("../network_config.toml"), config)
            .expect("Could not write network config to file");
    }
}

#[cfg(debug_assertions)]
const CONFIG_PATH: &str = "resources/config-debug.toml";
#[cfg(not(debug_assertions))]
const CONFIG_PATH: &str = "resources/config-production.toml";

/// expects a running swarm w/ the stack deployed
#[tokio::main]
async fn main() {
    env_logger::init();
    let config: Config =
        toml::from_str(&fs::read_to_string(CONFIG_PATH).expect("Could not read config file"))
            .expect("Could not parse config file");
    let docker = Docker::connect_with_unix(
        "/var/run/docker.sock",
        120,
        &ClientVersion {
            major_version: 1,
            minor_version: 41,
        },
    )
    .unwrap();
    let mut network_config = restart_system(&docker).await;
    for duration in &config.durations {
        for window_size_seconds in &config.window_size_seconds {
            for sampling_interval_ms in &config.sampling_interval_ms {
                for thread_pool_size in &config.thread_pool_sizes {
                    for no_motor_groups in &config.motor_groups_tcp {
                        scale_service(*no_motor_groups, &docker, &mut network_config).await;
                        for request_processing_model in &config.request_processing_models {
                            if *thread_pool_size as u32 <= 8
                                && *request_processing_model
                                    == RequestProcessingModel::ReactiveStreaming
                            {
                                continue; //not doable
                            }
                            let file_name = format!("{no_motor_groups}_{duration}_{window_size_seconds}_{sampling_interval_ms}_{thread_pool_size}_{}.csv", request_processing_model.to_string());
                            let mut file = OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(file_name.clone())
                                .unwrap();
                            let mut lines = fs::read_to_string(file_name).unwrap().lines().count();
                            if lines == 0 {
                                writeln!(file, "id,utime,ctime,cutime,cstime,vmhwm,vmpeak")
                                    .unwrap();
                                lines += 1;
                            }
                            for i in (lines - 1)..config.repetitions as usize {
                                info!("{i} {no_motor_groups} {duration} {window_size_seconds} {sampling_interval_ms} {thread_pool_size} {}", request_processing_model.to_string());
                                let results = execute_test_run(
                                    *no_motor_groups,
                                    *duration,
                                    *window_size_seconds,
                                    *sampling_interval_ms,
                                    *thread_pool_size,
                                    *request_processing_model,
                                );
                                match results {
                                    Ok(results) => write!(file, "{results}").unwrap(),
                                    Err(_) => {
                                        network_config = restart_system(&docker).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn setup_network_config(docker: &Docker) -> NetworkConfig {
    let mut cloud_socket_address = None;
    let mut monitor_socket_address = None;
    let containers = docker
        .inspect_network(
            "bench_system_default",
            None::<InspectNetworkOptions<String>>,
        )
        .await
        .expect("Could not get docker network")
        .containers
        .expect("Could not get docker network containers");
    for (_, container) in containers {
        let container_name = container.name.expect("Could not get container name");
        if container_name.contains("bench_system_cloud_server") {
            let address = container
                .ipv4_address
                .expect("Could not get container addresses");
            let addr = address.split('/').next().unwrap();
            cloud_socket_address =
                Some(IpAddr::from_str(addr).expect("Could not construct cloud server ip address"));
        } else if container_name.contains("bench_system_monitor") {
            let address = container
                .ipv4_address
                .expect("Could not get container addresses");
            let addr = address.split('/').next().unwrap();
            monitor_socket_address =
                Some(IpAddr::from_str(addr).expect("Could not construct motor monitor ip address"));
        }
    }
    NetworkConfig::new(
        cloud_socket_address.expect("Could not retrieve cloud server socket address"),
        monitor_socket_address.expect("Could not get motor monitor socket address"),
        get_sensor_ips(
            docker
                .inspect_network(
                    "bench_system_default",
                    None::<InspectNetworkOptions<String>>,
                )
                .await
                .unwrap(),
        ),
    )
}

async fn scale_service(no_motor_groups: u16, docker: &Docker, network_config: &mut NetworkConfig) {
    let execution_chain = docker
        .inspect_service("bench_system_sensor", None::<InspectServiceOptions>)
        .then(|current| {
            let mut current = current.unwrap();
            let options = UpdateServiceOptions {
                version: current.version.as_mut().unwrap().index.unwrap(),
                ..Default::default()
            };
            update_spec(no_motor_groups * 4, &mut current);
            docker
                .update_service("bench_system_sensor", current.spec.unwrap(), options, None)
                .then(|d| async move {
                    info!("{d:?}");
                    let mut sensor_ips = Vec::new();
                    while sensor_ips.len() != (no_motor_groups as usize) * 4 {
                        thread::sleep(Duration::from_secs(1));
                        let service_result = docker
                            .inspect_network(
                                "bench_system_default",
                                None::<InspectNetworkOptions<String>>,
                            )
                            .await
                            .unwrap();
                        sensor_ips = get_sensor_ips(service_result);
                    }
                    sensor_ips
                })
        });
    let ips: Vec<IpAddr> = execution_chain.await;
    network_config.update_sensor_addresses(ips);
}

fn get_sensor_ips(network: Network) -> Vec<IpAddr> {
    network
        .containers
        .unwrap()
        .iter()
        .filter(|(_, container)| {
            container
                .name
                .as_ref()
                .unwrap()
                .contains("bench_system_sensor")
        })
        .map(|(_, container)| container.ipv4_address.as_ref().unwrap().clone())
        .map(|ipv4_address| {
            let addr = ipv4_address.split('/').next().unwrap();
            IpAddr::from_str(addr).unwrap()
        })
        .collect()
}

fn update_spec(no_replicas: u16, current: &mut Service) {
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
        .replicas = Some(no_replicas.into());
}

fn execute_test_run(
    no_motor_groups: u16,
    duration: u64,
    window_size_seconds: u64,
    sampling_interval_ms: u32,
    thread_pool_size: usize,
    request_processing_model: RequestProcessingModel,
) -> Result<String, ()> {
    // let result = String::from_utf8(Command::new("cargo").output().unwrap().stdout).unwrap();
    // println!("{result}",);
    let mut command = Command::new("cargo");
    let mut child = command
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
        .spawn()
        .expect("Failure when trying to run test driver");
    thread::sleep(Duration::from_secs(duration));
    let mut process_finished = child.try_wait();
    for _ in 0..30 {
        if process_finished.is_ok() && process_finished.as_ref().unwrap().is_some() {
            break;
        }
        thread::sleep(Duration::from_secs(1));
        process_finished = child.try_wait();
    }
    if process_finished.is_err()
        || process_finished.as_ref().unwrap().is_none()
        || !process_finished.unwrap().unwrap().success()
    {
        Err(())
    } else {
        Ok(
            fs::read_to_string("../test_driver/motor_monitor_results.csv")
                .unwrap_or("".to_string()),
        )
    }
}

async fn restart_system(docker: &Docker) -> NetworkConfig {
    warn!("Restarting system");
    docker
        .list_containers::<String>(None)
        .then(|containers| async {
            let containers = containers.expect("Could not get containers");
            let container_names: Vec<&String> = containers
                .iter()
                .map(|container| {
                    container
                        .names
                        .as_ref()
                        .expect("Could not get container names")
                })
                .filter_map(|container_names| {
                    container_names.iter().find(|container_name| {
                        container_name.contains("bench_system")
                            && !container_name.contains("sensor")
                    })
                })
                .collect();
            for container_name in container_names.iter() {
                let container_name: String = container_name.chars().skip(1).collect();
                info!("Stopping {container_name}");
                let _ = docker.stop_container(&container_name, None).await;
            }
            for container_name in container_names.iter() {
                let mut i = 0;
                while i < 30 && !service_container_restarted(container_name, docker).await {
                    debug!("Waiting on {container_name} to run");
                    thread::sleep(Duration::from_secs(1));
                    i += 1;
                }
            }
        })
        .await;
    setup_network_config(docker).await
}

async fn service_container_restarted(container_name: &str, docker: &Docker) -> bool {
    let short_container_name = container_name
        .split('.')
        .next()
        .expect("Could not split c name");
    let containers = docker
        .list_containers::<String>(None)
        .await
        .expect("Could not list containers");
    containers.iter().any(|container| {
        container
            .names
            .as_ref()
            .expect("Could not get container names")
            .iter()
            .filter(|c_name| **c_name != container_name) //filter out old container
            .map(|c_name| c_name.split('.').next().expect("Could not split c name"))
            .any(|c_name| c_name == short_container_name)
    })
}
