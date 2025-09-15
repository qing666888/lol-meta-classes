# Multi-stage build for LoL Meta Classes sync
FROM rust:1.75-slim as builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy cargo files
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

# Build the applications
RUN cargo build --release --bin meta-sync --bin dumper --target x86_64-unknown-linux-gnu

# Runtime stage
FROM ubuntu:22.04

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy built binaries
COPY --from=builder /app/target/x86_64-unknown-linux-gnu/release/meta-sync ./meta-sync
COPY --from=builder /app/target/x86_64-unknown-linux-gnu/release/dumper ./target/x86_64-unknown-linux-gnu/release/dumper

# Create necessary directories
RUN mkdir -p dumps temp target/x86_64-unknown-linux-gnu/release

# Run the sync
CMD ["./meta-sync"]
