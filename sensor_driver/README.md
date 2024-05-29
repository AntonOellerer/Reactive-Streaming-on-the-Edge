# Sensor Driver

The is responsible for receiving new test run instructions and executing the
[sensor](../sensor) with the correct arguments.

## Execution

The sensor driver is executed with an address it should listen on as argument.
It then waits for incoming connections on the specified port.
Once a connection is established, it parses the benchmark run parameters and
executes the [sensor](../sensor) with the appropriate arguments.
After the [sensor](../sensor) finished, it starts waiting for incoming connections
anew.