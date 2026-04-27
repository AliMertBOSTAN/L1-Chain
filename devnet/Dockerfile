# Multi-stage Dockerfile for QuantumVault devnet image
# Stage 1: Builder
FROM rust:1.78-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    liboqs-dev \
    git \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /build

# Copy workspace and source
COPY . .

# Build workspace in release mode
RUN cargo build --release --bins \
    -p qv-node \
    -p qv-wallet \
    -p qv-miner \
    && strip target/release/qv-node \
    && strip target/release/qv-wallet \
    && strip target/release/qv-miner

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    liboqs \
    libssl3 \
    curl \
    jq \
    python3 \
    python3-flask \
    python3-pip \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install Flask and requests for Python scripts
RUN pip3 install --no-cache-dir \
    flask \
    requests \
    jinja2

# Create non-root user
RUN useradd -m -u 1000 qv && \
    mkdir -p /data /bootstrap /genesis && \
    chown -R qv:qv /data /bootstrap /genesis

# Copy binaries from builder
COPY --from=builder /build/target/release/qv-node /usr/local/bin/qv-node
COPY --from=builder /build/target/release/qv-wallet /usr/local/bin/qv-wallet
COPY --from=builder /build/target/release/qv-miner /usr/local/bin/qv-miner

# Set permissions on binaries
RUN chmod 755 /usr/local/bin/qv-* && \
    chown root:root /usr/local/bin/qv-*

# Switch to non-root user
USER qv

# Health check endpoint (basic curl check)
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:9944/health || exit 1

# Default entrypoint (can be overridden by docker-compose)
ENTRYPOINT ["qv-node"]
CMD ["--help"]
