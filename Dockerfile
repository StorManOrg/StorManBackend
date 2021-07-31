FROM rust:1.54 as builder
WORKDIR /usr/src/storagereloaded
COPY . .
RUN cargo install --path .

FROM debian:buster-slim
COPY --from=builder /usr/local/cargo/bin/storagereloaded /usr/local/bin/storagereloaded
CMD ["storagereloaded"]
