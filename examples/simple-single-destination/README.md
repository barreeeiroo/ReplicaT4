# Simple Single Destination Example

This example demonstrates ReplicaT4 proxying S3 requests to a single MinIO backend.

## Architecture

```
Client (AWS CLI/SDK)
        ↓
ReplicaT4 Proxy (:3000)
        ↓
   MinIO (:9000)
```

## What's Included

- **MinIO**: S3-compatible object storage backend
- **ReplicaT4**: S3 proxy server that forwards requests to MinIO
- **MinIO Console**: Web UI for MinIO (http://localhost:9001)

## Prerequisites

- Docker
- Docker Compose

## Quick Start

1. Navigate to this directory:
   ```bash
   cd examples/simple-single-destination
   ```

2. Start the services:
   ```bash
   docker-compose up --build
   ```

3. The services will be available at:
   - **ReplicaT4 Proxy**: http://localhost:3000
   - **MinIO API**: http://localhost:9000
   - **MinIO Console**: http://localhost:9001 (credentials: minioadmin/minioadmin)

## Testing

### Using AWS CLI

Configure AWS CLI to use the proxy:

```bash
# Configure AWS CLI (use any credentials, they're just for signature)
aws configure set aws_access_key_id test
aws configure set aws_secret_access_key test
aws configure set region us-east-1
```

Upload a file:
```bash
echo "Hello from ReplicaT4!" > test.txt
aws s3 cp test.txt s3://mybucket/test.txt --endpoint-url http://localhost:3000
```

List objects:
```bash
aws s3 ls s3://mybucket/ --endpoint-url http://localhost:3000
```

Download a file:
```bash
aws s3 cp s3://mybucket/test.txt downloaded.txt --endpoint-url http://localhost:3000
cat downloaded.txt
```

Delete a file:
```bash
aws s3 rm s3://mybucket/test.txt --endpoint-url http://localhost:3000
```

### Using curl

Upload a file:
```bash
curl -X PUT http://localhost:3000/mybucket/hello.txt \
  -H "Content-Type: text/plain" \
  -d "Hello World"
```

Get a file:
```bash
curl http://localhost:3000/mybucket/hello.txt
```

List objects:
```bash
curl http://localhost:3000/mybucket/
```

Delete a file:
```bash
curl -X DELETE http://localhost:3000/mybucket/hello.txt
```

## Verifying Data in MinIO

You can verify that data is actually stored in MinIO by:

1. Opening the MinIO Console at http://localhost:9001
2. Login with: `minioadmin` / `minioadmin`
3. Navigate to the `replicat4` bucket
4. You should see all objects uploaded through ReplicaT4

## Configuration

The `config.json` file configures:
- **virtualBucket**: The bucket name exposed by ReplicaT4 (`mybucket`)
- **backends**: List of S3-compatible backends (just MinIO in this example)

## Stopping the Services

```bash
docker-compose down
```

To also remove volumes (deletes all stored data):
```bash
docker-compose down -v
```

## Logs

View logs from all services:
```bash
docker-compose logs -f
```

View logs from specific service:
```bash
docker-compose logs -f replicat4
docker-compose logs -f minio
```
