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

- **Multi-Backend Replication**: Replicate data across multiple S3-compatible storage backends
- **Flexible Replication Modes**: Choose between async (fast) and synchronous (consistent) replication
- **Multiple Read Strategies**: From fast primary-only reads to consistency-verified reads across all backends
- **Streaming Architecture**: Zero-copy streaming for efficient handling of large objects
- **S3-Compatible API**: Drop-in replacement for S3-compatible applications

## Getting Started

For installation instructions, configuration options, and usage examples, please refer to the
[documentation](https://diego.barreiro.dev/ReplicaT4/).

## Acknowledgments

This tool was built with the assistance of Generative AI. All final decisions, content, and project direction were made
by the repository owner.

## License

[MIT LICENSE](LICENSE)
