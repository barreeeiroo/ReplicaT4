---
icon: material/cog
---

# Configuration

ReplicaT4 is configured using a JSON configuration file that defines your storage backends and replication behavior.
This page describes all available configuration options.

## Server Configuration

In addition to the JSON configuration file, ReplicaT4 accepts several command-line arguments and environment variables
for server configuration.

### Command-Line Arguments

```bash
replicat4 [OPTIONS] --config <config>

Options:
  -c, --config <config>
          Path to the configuration file (required)

  --host <host>
          Host to bind to [default: 0.0.0.0]

  -p, --port <port>
          Port to listen on [default: 3000]

  --access-key-id <access-key-id>
          AWS Access Key ID for incoming client requests
          [default: AKIAIOSFODNN7EXAMPLE]

  --secret-access-key <secret-access-key>
          AWS Secret Access Key for incoming client requests
          [default: wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY]

  -h, --help
          Print help
```

### Environment Variables

All command-line options can be set via environment variables:

| Environment Variable | Description | Default |
|---------------------|-------------|---------|
| `CONFIG_PATH` | Path to configuration file | (required) |
| `HOST` | Host to bind to | `0.0.0.0` |
| `PORT` | Port to listen on | `3000` |
| `AWS_ACCESS_KEY_ID` | Access key for client authentication | `AKIAIOSFODNN7EXAMPLE` |
| `AWS_SECRET_ACCESS_KEY` | Secret key for client authentication | `wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY` |

### Client Authentication

The `--access-key-id` and `--secret-access-key` options (or their environment variable equivalents) configure the credentials that **clients must use** to authenticate with ReplicaT4.

**Important**: These are **not** the credentials for backend storage services. Backend credentials are configured in the JSON configuration file.

**Example**:
```bash
replicat4 \
  --config config.json \
  --port 3000 \
  --access-key-id "MY_CUSTOM_KEY" \
  --secret-access-key "MY_CUSTOM_SECRET"
```

Clients would then connect using:
```bash
export AWS_ACCESS_KEY_ID="MY_CUSTOM_KEY"
export AWS_SECRET_ACCESS_KEY="MY_CUSTOM_SECRET"
aws s3 ls s3://mybucket/ --endpoint-url http://localhost:3000
```

## Configuration File

ReplicaT4 requires a configuration file to be specified when starting the server. The path to this file is provided
via the `--config` flag or the `CONFIG_PATH` environment variable.

```bash
replicat4 --config config.json
```

### Supported Formats

ReplicaT4 supports both **JSON** and **YAML** configuration file formats. The format is automatically detected based
on the file extension:

| Extension | Format |
|-----------|--------|
| `.json` | JSON |
| `.yaml` | YAML |
| `.yml` | YAML |

Extension detection is case-insensitive (e.g., `.JSON`, `.YAML`, `.YML` also work).

=== "JSON"
    ```json
    {
      "virtualBucket": "mybucket",
      "backends": [
        {
          "name": "aws-s3",
          "type": "s3",
          "region": "us-east-1",
          "bucket": "my-bucket"
        }
      ],
      "readMode": "PRIMARY_FALLBACK",
      "writeMode": "ASYNC_REPLICATION"
    }
    ```

=== "YAML"
    ```yaml
    virtualBucket: mybucket
    backends:
      - name: aws-s3
        type: s3
        region: us-east-1
        bucket: my-bucket
    readMode: PRIMARY_FALLBACK
    writeMode: ASYNC_REPLICATION
    ```

Both formats support the exact same configuration options. YAML can be more readable for complex configurations due to
its less verbose syntax.

### `virtualBucket`

**Type**: `string` (optional)

**Description**: The virtual bucket name that clients will use when connecting to ReplicaT4. If not specified,
defaults to `"mybucket"`.

**Example**:
```json
{
  "virtualBucket": "my-app-data"
}
```

When clients connect, they'll use this bucket name:
```bash
aws s3 ls s3://my-app-data/ --endpoint-url http://localhost:3000
```

---

### `readMode`

**Type**: `string` (required)

**Description**: Determines how ReplicaT4 reads data from backends. See [Read/Write Modes](read-write-modes.md) for
detailed behavior.

**Valid Values**:
- `"PRIMARY_ONLY"` - Read only from primary backend
- `"PRIMARY_FALLBACK"` - Try primary first, fallback on errors (recommended)
- `"BEST_EFFORT"` - Race all backends, return first response
- `"ALL_CONSISTENT"` - Verify all backends return consistent data

**Example**:
```json
{
  "readMode": "PRIMARY_FALLBACK"
}
```

---

### `writeMode`

**Type**: `string` (required)

**Description**: Determines how ReplicaT4 writes data to backends. See [Read/Write Modes](read-write-modes.md) for detailed behavior.

**Valid Values**:
- `"ASYNC_REPLICATION"` - Write to primary, replicate in background (fast)
- `"MULTI_SYNC"` - Write to all backends synchronously (consistent)

**Example**:
```json
{
  "writeMode": "ASYNC_REPLICATION"
}
```

---

### `primaryBackendName`

**Type**: `string` (optional)

**Description**: Explicitly specifies which backend to use as the primary. The name must match one of the backend `name` fields in the `backends` array.

**Mutually Exclusive With**: `useLatencyBasedPrimaryBackend`

**Default**: If not specified, the first backend in the `backends` array is used as primary.

**Example**:
```json
{
  "primaryBackendName": "aws-s3-primary",
  "backends": [
    {
      "name": "aws-s3-primary",
      ...
    },
    {
      "name": "backblaze-backup",
      ...
    }
  ]
}
```

---

### `useLatencyBasedPrimaryBackend`

**Type**: `boolean` (optional)

**Description**: When set to `true`, ReplicaT4 automatically selects the backend with the lowest latency as the primary
on startup. It performs 10 HEAD bucket requests to each backend and selects the one with the lowest median (P50)
latency.

**Mutually Exclusive With**: `primaryBackendName`

**Default**: `false`

**Example**:
```json
{
  "useLatencyBasedPrimaryBackend": true,
  "backends": [
    {
      "name": "aws-us-east",
      ...
    },
    {
      "name": "aws-eu-west",
      ...
    }
  ]
}
```

On startup, you'll see output like:
```
Backend 'aws-us-east' latency: 45ms (P50)
Backend 'aws-eu-west' latency: 120ms (P50)
Selected 'aws-us-east' as primary backend
```

---

### `backends`

**Type**: `array` (required)

**Description**: List of storage backends to replicate across. At least one backend must be configured.

**Backend Types**: Currently only `"s3"` type is supported, which works with any S3-compatible storage.

Each backend in the `backends` array must have `"type": "s3"` and the following fields:

---

#### `name`

**Type**: `string` (required)

**Description**: Unique identifier for this backend. Used in logs and for primary backend selection.

**Example**: `"aws-s3-primary"`, `"minio-local"`, `"backblaze-b2"`

---

#### `type`

**Type**: `string` (required)

**Description**: Backend type. Must be `"s3"`.

---

#### `region`

**Type**: `string` (required)

**Description**: AWS region or region identifier for the S3-compatible service.

**Examples**:

- AWS S3: `"us-east-1"`, `"eu-west-1"`
- Backblaze B2: `"us-west-004"`, `"eu-central-003"`
- MinIO: Any value, typically `"us-east-1"`
- DigitalOcean Spaces: `"nyc3"`, `"sfo3"`

---

#### `bucket`

**Type**: `string` (required)

**Description**: The actual bucket name on this backend storage service. This is the physical bucket that exists in
the S3-compatible service.

**Note**: This is different from `virtualBucket` (the bucket name clients use). ReplicaT4 maps the virtual bucket to
these physical buckets.

---

#### `endpoint`

**Type**: `string` (optional)

**Description**: Custom endpoint URL for S3-compatible services.

**When to Use**:
- Required for non-AWS services (MinIO, Backblaze B2, DigitalOcean Spaces, etc.)
- Optional for AWS S3 (uses default AWS endpoints if not specified)

**Examples**:

- MinIO: `"http://localhost:9000"` or `"https://minio.example.com"`
- Backblaze B2: `"https://s3.us-west-004.backblazeb2.com"`
- DigitalOcean Spaces: `"https://nyc3.digitaloceanspaces.com"`
- Wasabi: `"https://s3.us-east-1.wasabisys.com"`

---

#### `force_path_style`

**Type**: `boolean` (optional)

**Description**: Use path-style URLs instead of virtual-hosted-style URLs.

**Default**: `false`

**When to Set True**:
- MinIO (typically requires path-style)
- Local development environments
- Some S3-compatible services

**URL Styles**:
- Virtual-hosted style (default): `https://bucket-name.s3.amazonaws.com/object-key`
- Path style: `https://s3.amazonaws.com/bucket-name/object-key`

**Example**:
```json
{
  "name": "minio-local",
  "type": "s3",
  "endpoint": "http://localhost:9000",
  "force_path_style": true
}
```

---

#### `access_key_id`

**Type**: `string` (optional)

**Description**: Access key ID for authenticating with this backend.

**When to Specify**:
- Required if credentials are not available via environment variables or AWS credential chain
- Useful for different credentials per backend

**Default**: Uses AWS credential chain (environment variables, ~/.aws/credentials, IAM roles)

**Security Note**: Avoid committing credentials to version control. Use environment variable substitution or secret
management systems.

---

#### `secret_access_key`

**Type**: `string` (optional)

**Description**: Secret access key for authenticating with this backend.

**When to Specify**: Same as `access_key_id`

**Default**: Uses AWS credential chain

## Provider-Specific Examples

### AWS S3

```json
{
  "name": "aws-s3",
  "type": "s3",
  "region": "us-east-1",
  "bucket": "my-aws-bucket",
  "access_key_id": "AKIAIOSFODNN7EXAMPLE",
  "secret_access_key": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
}
```

### Backblaze B2

```json
{
  "name": "backblaze-b2",
  "type": "s3",
  "region": "us-west-004",
  "bucket": "my-b2-bucket",
  "endpoint": "https://s3.us-west-004.backblazeb2.com",
  "force_path_style": false,
  "access_key_id": "YOUR_B2_KEY_ID",
  "secret_access_key": "YOUR_B2_APPLICATION_KEY"
}
```

### MinIO

```json
{
  "name": "minio-local",
  "type": "s3",
  "region": "us-east-1",
  "bucket": "my-minio-bucket",
  "endpoint": "http://localhost:9000",
  "force_path_style": true,
  "access_key_id": "minioadmin",
  "secret_access_key": "minioadmin"
}
```

### DigitalOcean Spaces

```json
{
  "name": "do-spaces",
  "type": "s3",
  "region": "nyc3",
  "bucket": "my-space-name",
  "endpoint": "https://nyc3.digitaloceanspaces.com",
  "force_path_style": false,
  "access_key_id": "YOUR_SPACES_KEY",
  "secret_access_key": "YOUR_SPACES_SECRET"
}
```

### Wasabi

```json
{
  "name": "wasabi-storage",
  "type": "s3",
  "region": "us-east-1",
  "bucket": "my-wasabi-bucket",
  "endpoint": "https://s3.us-east-1.wasabisys.com",
  "force_path_style": false,
  "access_key_id": "YOUR_WASABI_KEY",
  "secret_access_key": "YOUR_WASABI_SECRET"
}
```

---

### Hetzner Object Storage

```json
{
  "name": "hetzner-storage",
  "type": "s3",
  "region": "fsn1",
  "bucket": "my-hetzner-bucket",
  "endpoint": "https://fsn1.your-objectstorage.com",
  "force_path_style": false,
  "access_key_id": "YOUR_HETZNER_KEY",
  "secret_access_key": "YOUR_HETZNER_SECRET"
}
```

**Available Regions**: `fsn1` (Falkenstein), `nbg1` (Nuremberg), `hel1` (Helsinki)

**Endpoint Format**: Replace `fsn1` with your region and use your project-specific endpoint from the Hetzner Cloud Console
