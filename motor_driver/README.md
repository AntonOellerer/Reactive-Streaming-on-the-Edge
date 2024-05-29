# Motor Driver

The motor driver waits for incoming connections from the [../test_driver](../test_driver)
receives from it the configuration for a single benchmark parameter set run, and
then executes the data stream processing service as appropriate.

## Execution

The motor driver is started before a benchmark is executed. It reads the port address
it should listen on for alerts from the file in [resources](resources)
corresponding to whether it is run in debug or production mode, and then waits
for test run start information arriving from the [Test Driver](../test_driver).
Once it receives such instructions, it first forwards the appropriate part of the
instructions to the [Sensor Driver](../sensor_driver) (This is done so that no
connection between the test driver and the sensors driver needs to be established).
Afterward, it executes the data stream processing service
specified in the test run information (by the `request_processing_model` field),
passing it the necessary program arguments, and then waits for its completion.
Upon execution of the data stream processing service, its `stdout` is configured
to write directly to the connection to the test driver, so that the performance
metrics are directly forwarded.