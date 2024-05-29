# Sensor

The Sensor is the component emulating the IoT sensors measuring properties
about a motor.

Depending on the ID it was passed upon execution, it takes its readings from
one of the files in [resources](resources) in a random pattern, where the
seed is initialized with the sensor ID.

## Execution

Upon execution, the sensor reads its configuration from the arguments:

1. id: `u32`
2. start_time: `f64`
3. duration: `f64`
4. sampling_interval: `u32`
5. ignored: `String`
6. motor_monitor_listen_address: `SocketAddr`

It then initializes a random number generator with its `id` as seed, and starts
reading values randomly from the file in [resources](resources) corresponding
to its `id % 4`.
Each reading is sent to the data stream processor at the `motor_monitor_listen_address`.
After the `duration` has elapsed, the process exits.