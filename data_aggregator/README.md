# Data Aggregator

The Data Aggregator can be executed to create diagrams and run statistical analyses on the data collected during
benchmark execution.

## Execution

After a benchmark suite execution, the data aggregator can be invoked from the
command line.
As arguments, it expects between one and three integers, which specify the
inner x-axis, the outer x-axis, and the outer y-axis that should be used for
analysing the data. The integers are indices referring to the token in the file
names of the benchmark results where

0. the number of motor groups
1. the benchmark run duration
2. the window size
3. the window sampling interval
4. the sensor sampling interval
5. the thread pool size

(for the data analysis of the thesis, the aggregator was run with the arguments `2 0`, representing
the window size and the number of motor groups).

Upon execution, the metrics are read from the CSV files in [../bench_executor](../bench_executor).
Additional to the 6 parts specified above, the files are either ending in `ru` or `ad`, signifying
whether they contain the collected `resource usage` or `alert delays`.

The metrics are used for creating aggregated CSV files of
the alert delays, load average, memory usage, and the processing time, which
are named following the pattern `{metric_name}_{motor_groups}_{processing_model}`.

Furthermore, boxplots are created depicting the performance of the stream data
processing services graphically.

Additionally, t-tests are done to check whether the differences in means per
parameter set between the two processing models are significant.

The Data Aggregator found in the
branch [feature/two_data_sources](https://github.com/AntonOellerer/Reactive-Streaming-on-the-Edge/tree/feature/two_data_sources)
has been modified slightly to allow the comparison of benchmarking suite results
collected from two different suite executions (on two different systems).