# Imperative Data Stream Processing Service

The imperative data stream processing service is one of the two services which
have been benchmarked for my thesis.

The service receives data from multiple sensors, where four sensors each are
connected to a (virtual) motor, slices it into time-based windows, averages per
sensors, and then combines them. If the combined values exceed the 90% CI of the
expected value, an alert is sent to the cloud server.

Software-architecture wise, the service creates one thread per sensor and
one per motor, where the "sensor threads" are each connected to one sensor via
TCP, and the four threads connected to the sensors of one motor are again
connected to a fifth thread.
Upon arrival of a sensor reading, the "sensor thread" puts the value into a list,
checks whether the time has passed to forward another average, and does so
if necessary.
The "motor thread" receives the averages, combines them, and sends an alert to the
cloud sensor if necessary.

## Execution

The imperative data stream processing service expects the following arguments upon execution:

1. start_time: `f64`
2. duration: `f64`
3. request_processing_model: `String`
4. number_of_tcp_motor_groups: `usize`
5. ignored: `u8`
6. window_size_ms: `u64`
7. sensor_listen_address: `String`
8. motor_monitor_listen_address: `String`
9. sensor_sampling_interval: `u32`
10. window_sampling_interval: `u32`
11. ignored: `usize`

It then starts listening on the `sensor_listen_address` for incoming connections
from sensors.
Once data is being sent, it processes it according to the specified rules, and
sends alert to the `motor_monitor_listen_address`.

Once execution has finished, it retrieves the metrics from the `/proc/{pid}` subsystem,
writes them to `stdout`, and exits.

The following metrics are collected:

1. id: `u32`,
2. time_spent_in_user_mode: `u64`,
3. time_spent_in_kernel_mode: `u64`,
4. children_time_spent_in_user_mode: `u64`,
5. children_time_spent_in_kernel_mode: `u64`,
6. peak_resident_set_size: `u64`,
7. peak_virtual_memory_size: `u64`,
8. load_average: `f32`,
9. benchmark_data_type: `String`,
