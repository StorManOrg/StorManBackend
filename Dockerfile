FROM rust:1.58.1 as builder
WORKDIR /usr/src/storagereloaded
COPY . .
RUN cargo install --path .

FROM debian:bullseye-slim
COPY --from=builder /usr/local/cargo/bin/storagereloaded /usr/local/bin/storagereloaded
CMD ["storagereloaded"]
