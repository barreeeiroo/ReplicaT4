---
icon: material/rocket-launch
---

# Getting Started

This guide will help you install and run ReplicaT4 for the first time.

## Prerequisites

Before installing ReplicaT4, ensure you have:

- **Docker** installed on your system
- **At least one S3-compatible storage backend** (AWS S3, MinIO, Backblaze B2, etc.)
- **Access credentials** for your storage backend(s)

## Quick Start with Docker

The fastest way to get started is using Docker.

### 1. Create a Configuration File

Create a file named `config.json`:

```json
{
  "virtualBucket": "mybucket",
  "readMode": "PRIMARY_FALLBACK",
  "writeMode": "ASYNC_REPLICATION",
  "backends": [
    {
      "type": "s3",
      "name": "primary",
      "region": "us-east-1",
      "bucket": "your-bucket-name",
      "access_key_id": "YOUR_ACCESS_KEY",
      "secret_access_key": "YOUR_SECRET_KEY"
    }
  ]
}
```

Replace `your-bucket-name`, `YOUR_ACCESS_KEY`, and `YOUR_SECRET_KEY` with your actual S3 credentials.

### 2. Run ReplicaT4 with Docker

```bash
docker run -d \
  --name replicat4 \
  -p 3000:3000 \
  -v $(pwd)/config.json:/app/config.json:ro \
  -e AWS_ACCESS_KEY_ID=MYKEY \
  -e AWS_SECRET_ACCESS_KEY=MYSECRET \
  ghcr.io/barreeeiroo/replicat4:latest
```

**Parameters**:
- `-p 3000:3000`: Expose port 3000
- `-v $(pwd)/config.json:/app/config.json:ro`: Mount your config file (read-only)
- `-e AWS_ACCESS_KEY_ID=MYKEY`: Credentials for clients to authenticate with ReplicaT4
- `-e AWS_SECRET_ACCESS_KEY=MYSECRET`: Secret key for client authentication

### 3. Verify It's Running

Check the logs:
```bash
docker logs replicat4
```

You should see:
```
Loaded configuration from /app/config.json
Using bucket: mybucket
Initializing S3 backend: primary
Server listening on 0.0.0.0:3000
```

### 4. Test with AWS CLI

```bash
export AWS_ACCESS_KEY_ID=MYKEY
export AWS_SECRET_ACCESS_KEY=MYSECRET

aws s3 ls s3://mybucket/ --endpoint-url http://localhost:3000
```

Success! You're now running ReplicaT4.

---

## Configuration Examples

### Create Your Configuration File

Create a `config.json` file with your backends. Here's a minimal example:

```json
{
  "virtualBucket": "mybucket",
  "readMode": "PRIMARY_FALLBACK",
  "writeMode": "ASYNC_REPLICATION",
  "backends": [
    {
      "type": "s3",
      "name": "primary-backend",
      "region": "us-east-1",
      "bucket": "my-actual-bucket"
    }
  ]
}
```

For multi-backend setups:

```json
{
  "virtualBucket": "mybucket",
  "readMode": "PRIMARY_FALLBACK",
  "writeMode": "MULTI_SYNC",
  "primaryBackendName": "aws-primary",
  "backends": [
    {
      "type": "s3",
      "name": "aws-primary",
      "region": "us-east-1",
      "bucket": "my-aws-bucket"
    },
    {
      "type": "s3",
      "name": "backblaze-backup",
      "region": "us-west-004",
      "bucket": "my-b2-bucket",
      "endpoint": "https://s3.us-west-004.backblazeb2.com",
      "access_key_id": "YOUR_B2_KEY",
      "secret_access_key": "YOUR_B2_SECRET"
    }
  ]
}
```

See the [Configuration](configuration.md) page for detailed configuration options.

---

## Testing Your Setup

### 1. Upload a Test File

Create a test file:
```bash
echo "Hello from ReplicaT4" > test.txt
```

Upload it:
```bash
aws s3 cp test.txt s3://mybucket/test.txt --endpoint-url http://localhost:3000
```

### 2. Verify in Backend Storage

Check that the file exists in your actual backend bucket:

```bash
# For AWS S3 backend
aws s3 ls s3://your-actual-bucket-name/test.txt

# Or via web console
# Navigate to your S3/B2/Spaces bucket and verify the file is there
```

### 3. Download the File

Download through ReplicaT4:
```bash
aws s3 cp s3://mybucket/test.txt downloaded.txt --endpoint-url http://localhost:3000
cat downloaded.txt
```

You should see: `Hello from ReplicaT4`

### 4. Test Multi-Backend Replication (if configured)

If you have multiple backends configured:

1. Upload a file through ReplicaT4
2. Check each backend to verify the file was replicated

```bash
# Upload via ReplicaT4
aws s3 cp myfile.txt s3://mybucket/myfile.txt --endpoint-url http://localhost:3000

# Check first backend
aws s3 ls s3://backend1-bucket/myfile.txt

# Check second backend
aws s3 ls s3://backend2-bucket/myfile.txt --endpoint-url https://your-second-backend.com
```

---

## Running with Docker Compose

For production deployments or local development with multiple services:

Create a `docker-compose.yml`:

```yaml
version: '3.8'

services:
  replicat4:
    image: ghcr.io/barreeeiroo/replicat4:latest
    container_name: replicat4
    ports:
      - "3000:3000"
    volumes:
      - ./config.json:/app/config.json:ro
    environment:
      - CONFIG_PATH=/app/config.json
      - PORT=3000
      - AWS_ACCESS_KEY_ID=MYKEY
      - AWS_SECRET_ACCESS_KEY=MYSECRET
      - RUST_LOG=replicat4=info
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "pidof", "replicat4"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 5s
```

Run it:
```bash
docker-compose up -d
```

View logs:
```bash
docker-compose logs -f replicat4
```

---

## Next Steps

Now that you have ReplicaT4 running:

1. **Learn about [Read/Write Modes](read-write-modes.md)** to optimize performance and consistency
2. **Review [Configuration](configuration.md)** options for advanced setups
3. **Explore [Usage Examples](usage-examples.md)** for common scenarios
4. **Deploy to production** using Docker or your preferred deployment method

---

## Getting Help

If you encounter issues:

1. Review logs: `docker logs replicat4`
2. Check the [Configuration](configuration.md) documentation
3. Open an issue on [GitHub](https://github.com/barreeeiroo/ReplicaT4/issues)
