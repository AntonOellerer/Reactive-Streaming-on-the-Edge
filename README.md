# Reactive Streaming on the Edge

This repository contains the code I developed for my diploma thesis 

**Efficent processing of IoT data streams -  
Evaluating imperative and declarative programming in resource-constrained environments**

## Scenario
The scenario developed to answer the research questions posed in this thesis is
defined as follows:

A motor is measured by four sensors. Their readings are sent to a data stream processor, which
combines them and sends an alert to a cloud server if the compounded value exceed the expected
90% confidence interval.

## Components
The repository contains two(+1) main types of components:
* Components used for executing the benchmark
  * [Bench Executor](bench_executor)
  * [Test Driver](test_driver)
  * [Motor Driver](motor_driver)
  * [Sensor Driver](sensor_driver)
  * [Sensor](sensor)
  * [Cloud Server](cloud_server)
  * [Data Transfer Objects](data_transfer_objects)
  * [Scheduler](scheduler)
  * [Utils](utils)
  * [Data Aggregator](data_aggregator)
* Components constituting services which are benchmarked
  * [Imperative Data Stream Processing Service](motor_monitor_oo)
  * [Declarative Data Stream Processing Service](motor_monitor_rx)

((
* Remnants from earlier experimentation, which did not make it into the final system
  * [A simple client-server-based Data Stream Processing Service](motor_monitor_cs)
  * [A Data Stream Processing Service using the SpringQL project](motor_monitor_sql)
  * [A sensor emulator implementation which runs on the Raspberry Pi Pico](pico_sensor)

))

## Architecture
The [Sensor Driver](sensor_driver) and the [Cloud Server](cloud_server) run as docker services on the executing
machine, the  [Motor Driver](motor_driver) is a docker service running on the resource constrained SBC, in the case
of the thesis a Raspberry Pi B3.

The [Bench Executor](bench_executor) is responsible for the full execution of the benchmark
suite.
It scales the number of [Sensor Drivers](sensor_driver) to the number necessary for each benchmark run,
executes the [Test Driver](test_driver) once the run can begin, and collects the collected
performance metrics afterwards.  
Furthermore, it restarts the [Motor Driver](motor_driver) and the [Cloud Server](cloud_server), and restarts them in the event of a
failure.

The [Test Driver](test_driver) is responsible for executing a single benchmark run.
It parses the data received from the [Bench Executor](bench_executor), distributes it as required to the
[Motor Driver](motor_driver), [Sensor Driver](sensor_driver), and the [Cloud Server](cloud_server), and
afterward expects the performance metrics collected at the SBC and the [Cloud Server](cloud_server).

The [Motor Driver](motor_driver) runs as a single replication on the SBC, waiting for test run
instructions. Once it receives them, it executes the required data stream processing service, waits
for its completion, and returns the collected metrics to the [Test Driver](test_driver) afterwards.

Similarly, the [Sensor Driver](sensor_driver) runs as many times as the benchmark run requires, waits for data
arriving from the [Test Driver](test_driver) and then starts the [Sensor](sensor) with the necessary information.

The [Sensor](sensor) is executed with the information describing a single benchmarking
run, proceeds to read the virtual sensor data readings from the appropriate file in
[sensor/resources](sensor/resources), and exits afterward.

The [Cloud Server](cloud_server) waits until it receives the parameters for a 
benchmarking run. It then starts listening on a predefined port, collecting
all alerts sent by the data stream processor, timestamping them on arrival.
After the connection is closed by the data stream processor, it sends the
collected alerts to the [Test Driver](test_driver) and waits for the start
of the next run.

## Related Projects
The dataset presented in _Explainable Artificial Intelligence for Predictive Maintenance Applications_ by Stephan Matzka
([DOI: 10.1109/AI4I49448.2020.00023](https://doi.org/10.1109/AI4I49448.2020.00023)) is used for the virtual motor readings
collected by the sensors.

[rx_rust_mp](https://github.com/AntonOellerer/rx_rust_mp) was also created over the course of this thesis, and is used by
the [Declarative Data Stream Processing Service](motor_monitor_rx) for handling the data-infrastructural tasks of its
implementation.