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
# 健康检查:relay 是纯 WS 服务,未知路径一律静默关闭,没有可探测的 HTTP 路由;
# bookworm-slim 也没有 curl/wget。所以用 bash 内建的 /dev/tcp 做端口级探测:
# 能建立 TCP 连接即视为存活。探测连接不会完成 WS 握手,10s 握手超时后被服务端
# 回收;不经过 client_connect 的限流,也只短暂占用连接槽位。
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/${RELAY_PORT:-6791}"]
CMD ["aeroric-remote-relay"]
