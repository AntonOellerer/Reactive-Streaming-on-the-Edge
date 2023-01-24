FROM rust:1.66.1 as builder
WORKDIR /usr/src/sensor_driver
COPY . .
RUN cargo install --path

WORKDIR /usr/src/sensor
COPY ../sensor .
RUN cargo install --path

FROM debian:buster-slim
COPY --from=builder /usr/local/cargo/bin/sensor_driver /usr/local/bin/sensor_driver
COPY --from=builder /usr/local/cargo/bin/sensor /usr/local/bin/sensor
CMD ["sensor_driver"]