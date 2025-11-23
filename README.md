# ReplicaT4

[![CI](https://github.com/barreeeiroo/ReplicaT4/actions/workflows/ci.yml/badge.svg)](https://github.com/barreeeiroo/ReplicaT4/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/barreeeiroo/ReplicaT4/branch/main/graph/badge.svg)](https://codecov.io/gh/barreeeiroo/ReplicaT4)

> ⚠ PROJECT UNDER DEVELOPMENT ⚠

An **S3-compatible proxy server that intercepts and replicates object storage operations across multiple
backends** simultaneously. Supports any S3-compatible storage (AWS S3, MinIO, Backblaze B2, etc.) as
replication targets.

## Why ReplicaT4?

ReplicaT4 provides replication **independently of whether your storage providers have native replication features**. It
acts as a transparent wrapper that enables **multi-cloud replication** across any S3-compatible storage backends without
depending on vendor-specific capabilities.

## Features

- **Multi-Backend Replication** - Replicate across any S3-compatible storage (AWS S3, MinIO, Backblaze B2, etc.)
- **Flexible Write Modes** - Async replication (fast) or multi-sync (consistent)
- **Smart Read Strategies** - Primary-only, fallback, best-effort, or all-consistent modes
- **Streaming Architecture** - Zero-copy streaming for efficient large object handling
- **Drop-in Replacement** - Standard S3 API, no application code changes required

## Quick Start

```bash
# 1. Create config.json with your backends
# 2. Run with Docker
docker run -d -p 3000:3000 \
  -v $(pwd)/config.json:/app/config.json:ro \
  -e AWS_ACCESS_KEY_ID=MYKEY \
  -e AWS_SECRET_ACCESS_KEY=MYSECRET \
  ghcr.io/barreeeiroo/replicat4:latest

# 3. Use standard S3 tools
aws s3 ls s3://mybucket/ --endpoint-url http://localhost:3000
```

## Documentation

- **[Getting Started](https://diego.barreiro.dev/ReplicaT4/getting-started/)** - Installation and setup guide
- **[Motivation](https://diego.barreiro.dev/ReplicaT4/motivation/)** - Why ReplicaT4 exists and the problem it solves
- **[Configuration](https://diego.barreiro.dev/ReplicaT4/configuration/)** - Complete configuration reference
- **[Read/Write Modes](https://diego.barreiro.dev/ReplicaT4/read-write-modes/)** - Understanding replication strategies
- **[Usage Examples](https://diego.barreiro.dev/ReplicaT4/usage-examples/)** - Common usage scenarios

## Acknowledgments

This tool was built with the assistance of Generative AI. All final decisions, content, and project direction were made
by the repository owner.

## License

[MIT LICENSE](LICENSE)
