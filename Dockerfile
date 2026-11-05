# syntax=docker/dockerfile:1
ARG RUST_VERSION=1.99.1
ARG ALPINE_VERSION=3.24.1
ARG GIT_COMMIT_HASH

ARG APP_NAME=LittleTuffTracker

################################################################################
FROM rust:${RUST_VERSION}-alpine AS build
ARG APP_NAME
WORKDIR /app
COPY . .

RUN apk add --no-cache clang lld musl-dev openssl-libs-static git perl make curl protoc
ENV RUSTFLAGS="-C link-arg=-fuse-ld=lld"
ARG GIT_COMMIT_HASH
ENV GIT_COMMIT_HASH=$GIT_COMMIT_HASH

ENV SQLX_OFFLINE=true
RUN --mount=type=cache,target=/app/target/,id=${APP_NAME} \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
cargo build --locked --release -p LittleTuffTracker && \
cp ./target/release/$APP_NAME /bin/$APP_NAME

################################################################################
FROM alpine:${ALPINE_VERSION} AS final

ARG UID=10001
RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    appuser

COPY --from=build /bin/LittleTuffTracker /bin/

RUN mkdir -p /app/logs
RUN chown -R appuser:appuser /app/logs

USER appuser
ENV RUST_LOG=info
ENV LOG_DIR=/app/logs/
EXPOSE 6969/udp

CMD ["/bin/LittleTuffTracker"]
