# Aeroric 自托管 relay 镜像。
# 构建上下文必须是仓库根目录(依赖 remote-protocol crate):
#   docker build -f remote-relay/Dockerfile -t aeroric-remote-relay .
# 运行:
#   docker run -d -p 6791:6791 -e RELAY_TOKEN=<共享口令> aeroric-remote-relay

FROM rust:1-slim AS build
WORKDIR /build
COPY remote-protocol ./remote-protocol
COPY remote-relay ./remote-relay
WORKDIR /build/remote-relay
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=build /build/remote-relay/target/release/aeroric-remote-relay /usr/local/bin/aeroric-remote-relay
ENV RELAY_PORT=6791
EXPOSE 6791
USER nobody
CMD ["aeroric-remote-relay"]
