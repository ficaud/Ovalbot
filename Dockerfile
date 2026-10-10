# syntax=docker/dockerfile:1

# ---------- Stage 1 : build frontend (SvelteKit, adapter-static) ----------
FROM node:24-alpine AS frontend
WORKDIR /app/frontend
COPY frontend/package*.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# ---------- Stage 2 : build backend (Rust, release) ----------
FROM rust:1-slim AS builder
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY calendar ./calendar
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release

# ---------- Stage 3 : image runtime minimale ----------
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/ovalbot /usr/local/bin/ovalbot
COPY --from=builder /app/target/release/create_admin /usr/local/bin/create_admin
COPY --from=frontend /app/frontend/build /app/static
RUN useradd --system --uid 10001 ovalbot
RUN mkdir -p /data && chown -R ovalbot:ovalbot /data
USER ovalbot
EXPOSE 3000
ENV DATABASE_URL=sqlite:///data/ovalbot.sqlite
ENV STATIC_DIR=/app/static
VOLUME ["/data"]
CMD ["/usr/local/bin/ovalbot"]
