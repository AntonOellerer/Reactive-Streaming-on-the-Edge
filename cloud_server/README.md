# Cloud Server

The Cloud Server emulates a server receiving alerts from an
edge device.

## Execution

The cloud server is started before a benchmark is
executed. It reads the port address it should listen on for alerts from the
file in [resources](resources) corresponding to whether it is run in debug or
production mode, and then waits for test run start information arriving from the
[Test Driver](../test_driver)
It then starts listening on the specified port, collecting
all alerts sent by the data stream processor, timestamping them on arrival.
After the connection is closed by the data stream processor, it sends the
collected alerts to the [Test Driver](../test_driver) and waits for the start
of the next run.