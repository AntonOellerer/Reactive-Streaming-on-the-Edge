FROM rust:1.66.1 as builder
WORKDIR /usr/src/cloud_server
COPY . .
RUN cargo install --path

FROM debian:buster-slim
COPY --from=builder /usr/local/cargo/bin/cloud_server /usr/local/bin/cloud_server
CMD ["cloud_server"]