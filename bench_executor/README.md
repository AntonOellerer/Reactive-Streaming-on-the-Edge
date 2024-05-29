# Bench Executor

The Bench Executor is responsible for the full execution of the benchmark
suite.
It scales the number of [Sensor Drivers](../sensor_driver) to the number necessary for each benchmark run,
executes the [Test Driver](../test_driver) once the run can begin, and collects the collected
performance metrics afterward.  
Furthermore, it restarts the [Motor Driver](../motor_driver) and the [Cloud Server](../cloud_server), and restarts them
in the event of a
failure.

## Execution

The executor is started through the command line, with no program arguments being necessary.
The exact configuration of a full benchmark execution is specified via the
configuration file.
Depending on whether the executor is run in debug mode or not, [config-debug.toml](resources/config-debug.toml)
or [config-production.toml](resources/config-production.toml) is read upon startup,
which configures the following parameters:

* `inner_repetitions`: How many times each parameter set should be executed in series
* `motor_groups_tcp`: An array specifying the different number of motor groups the system should be benchmarked with
* `window_size_ms`: An array specifying the different sizes (in ms) of the data windows the system should be benchmarked
  with
* `window_sampling_interval_ms`: An array specifying the different window sampling intervals (in ms) the system should
  be benchmarked with
* `sensor_sampling_interval_ms`: An array specifying the different sensor sampling intervals (in ms) the system should
  be benchmarked with
* `request_processing_models`: An array specifying the different data stream services that should be benchmarked
* `outer_repetitions`: How many times the set of the parameters above should be executed.

The reason `inner_repetitions` and `outer_repetitions` exists is to strike a balance between the rescaling of the
system,
and diminishing the impact of longer disturbances of the benchmarking system. (e.g. if the internet stops working for 20
minutes,
instead of all benchmarking runs of one parameter set being wrong, the invalid runs are distributed over more parameter
combinations.)

Upon startup, the Bench Executor expects the `bench_system_monitor`, the `bench_system_cloud_server`, and the
`bench_system_sensor` docker services to be running, the two first one with a replication of one.
It then executes the benchmarking run, and persists the collected metrics in CSV files named following the pattern
`{no_motor_groups}_{run_duration}_{window_size}_{window_sampling_interval}_{sensor_sampling_interval}_{thread_pool_size}_{request_processing_model}_{dataset}`
where `dataset` is either `ru` for resource usage or `ad` for alert delays.

During execution, if a run fails, it restarts the system by scaling the docker services to 0 and then back to
the required amount of replications.