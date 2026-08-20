# ---------- 构建阶段 ----------
FROM rust:1.92-slim AS builder
WORKDIR /app

# rusqlite bundled 需要 C 编译器
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential \
    && rm -rf /var/lib/apt/lists/*

# 先拷贝依赖清单，最大化利用构建缓存
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main(){}' > src/main.rs \
    && cargo build --release \
    && rm -rf src

# 拷贝源码并真正编译
COPY src ./src
RUN touch src/main.rs && cargo build --release

# ---------- 运行阶段 ----------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /data
COPY --from=builder /app/target/release/btboy /usr/local/bin/btboy

ENV DB_PATH=/data/btboy.db
VOLUME ["/data"]

CMD ["btboy"]
