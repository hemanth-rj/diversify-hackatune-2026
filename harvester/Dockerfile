# ---- build stage ----
# build-essential supplies gcc for aubio's bundled C library; the required
# CFLAGS workaround lives in .cargo/config.toml and is picked up automatically.
FROM rust:1-bookworm AS build
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends build-essential \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release --bin harvest

# ---- runtime stage ----
# ffmpeg = decode fallback for VBR/odd MP3s; ca-certificates = TLS to Jamendo.
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /app/target/release/harvest /usr/local/bin/harvest
ENTRYPOINT ["harvest"]
