# Build stage
FROM rust:1.91-alpine AS builder

# Install build dependencies
RUN apk add --no-cache \
    musl-dev \
    openssl-dev \
    pkgconfig

# Create a new empty project
WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Build for release
RUN cargo build --release

# Runtime stage
FROM alpine:3.19

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    libgcc

# Create a non-root user
RUN adduser -D -u 1000 replicat4

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/replicat4 /usr/local/bin/replicat4

# Change ownership
RUN chown -R replicat4:replicat4 /app

# Switch to non-root user
USER replicat4

# Expose the default port
EXPOSE 3000

# Set default environment variables
ENV CONFIG_PATH=/app/config.json \
    PORT=3000 \
    RUST_LOG=replicat4=info

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD pidof replicat4 || exit 1

# Run the application
CMD ["replicat4", "--config", "/app/config.json"]
