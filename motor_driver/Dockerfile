FROM rust:1.66.1 as builder
WORKDIR /usr/src/motor_driver
COPY . .
RUN cargo install --path

WORKDIR /usr/src/motor_monitor_cs
COPY ../motor_monitor_cs .
RUN cargo install --path

WORKDIR /usr/src/motor_monitor_rx
COPY ../motor_monitor_rx .
RUN cargo install --path

FROM debian:buster-slim
COPY --from=builder /usr/local/cargo/bin/motor_driver /usr/local/bin/motor_driver
COPY --from=builder /usr/local/cargo/bin/motor_monitor_cs /usr/local/bin/motor_monitor_cs
COPY --from=builder /usr/local/cargo/bin/motor_monitor_rx /usr/local/bin/motor_monitor_rx
CMD ["cloud_server"]