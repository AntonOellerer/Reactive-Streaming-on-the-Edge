# Test Driver

The Test Driver coordinates the execution of a single benchmark parameter set
run.

It receives the parameters and the system network layout, forwards the configuration
as necessary, and then waits for the arrival of the performance metrics.

## Execution
The Test Driver receives its configuration both as program arguments
(the parameters of the test run) and via a config file, which describes the
network layout of the system (where the components are located).

It then partitions the parameters into the appropriate data transfer objects and
transmits them to the [cloud server](../cloud_server) and the [motor driver](../motor_driver)
(which again forwards a part to the [sensor driver](../sensor_driver)).

It then waits the specified time, and reads the data stream processors performance metrics from
its connection to the [motor driver](../motor_driver), persisting them to a file.
After that, it receives the alert delays from the [cloud server](../cloud_server),
saves them to a file as well, and exits.