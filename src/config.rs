use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReadMode {
    /// Only read from primary backend (or first if no primary specified)
    PrimaryOnly,
    /// Try primary first, fallback to other backends on failure
    PrimaryFallback,
    /// Try any backend to find the object
    BestEffort,
    /// Read from all backends, verify consistency (ETags match), return primary result
    AllConsistent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WriteMode {
    /// Write to primary/first backend immediately, replicate to others asynchronously
    AsyncReplication,
    /// Write to all backends synchronously, all must succeed
    MultiSync,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_bucket: Option<String>,
    pub read_mode: ReadMode,
    pub write_mode: WriteMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_backend_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub use_latency_based_primary_backend: Option<bool>,
    pub backends: Vec<BackendConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum BackendConfig {
    S3(S3BackendConfig),
    Memory(MemoryBackendConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3BackendConfig {
    pub name: String,
    pub region: String,
    pub bucket: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub force_path_style: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_access_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryBackendConfig {
    pub name: String,
}

impl BackendConfig {
    pub fn name(&self) -> &str {
        match self {
            BackendConfig::S3(s3) => &s3.name,
            BackendConfig::Memory(mem) => &mem.name,
        }
    }
}

/// Supported configuration file formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Json,
    Yaml,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        let format = Self::detect_format(path)?;
        let content = fs::read_to_string(path)?;
        let config = Self::parse_content(&content, format)?;
        config.validate()?;
        Ok(config)
    }

    fn detect_format(path: &Path) -> Result<ConfigFormat, Box<dyn std::error::Error>> {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase());

        match extension.as_deref() {
            Some("json") => Ok(ConfigFormat::Json),
            Some("yaml") | Some("yml") => Ok(ConfigFormat::Yaml),
            Some(ext) => Err(format!(
                "Unsupported config file extension: '.{}'. Supported: .json, .yaml, .yml",
                ext
            )
            .into()),
            None => Err("Config file must have an extension (.json, .yaml, or .yml)".into()),
        }
    }

    fn parse_content(content: &str, format: ConfigFormat) -> Result<Self, Box<dyn std::error::Error>> {
        let config: Config = match format {
            ConfigFormat::Json => serde_json::from_str(content)?,
            ConfigFormat::Yaml => serde_yml::from_str(content)?,
        };
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Check that all backend names are unique
        let mut seen_names = HashSet::new();
        for backend in &self.backends {
            let name = backend.name();
            if !seen_names.insert(name) {
                return Err(format!("Duplicate backend name: {}", name).into());
            }
        }

        // Check that primaryBackendName and useLatencyBasedPrimaryBackend are mutually exclusive
        if self.primary_backend_name.is_some()
            && self.use_latency_based_primary_backend == Some(true)
        {
            return Err(
                "Cannot specify both primaryBackendName and useLatencyBasedPrimaryBackend".into(),
            );
        }

        // Check that primaryBackendName, if specified, exists in backends
        if let Some(primary_name) = &self.primary_backend_name {
            let exists = self.backends.iter().any(|b| b.name() == primary_name);
            if !exists {
                return Err(format!(
                    "Primary backend name '{}' not found in backends list",
                    primary_name
                )
                .into());
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_s3_backend_minimal() {
        let json = r#"{
            "backends": [
                {
                    "type": "s3",
                    "name": "aws-s3",
                    "region": "us-east-1",
                    "bucket": "my-bucket"
                }
            ],
            "readMode": "PRIMARY_FALLBACK",
            "writeMode": "ASYNC_REPLICATION"
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.backends.len(), 1);
        assert_eq!(config.read_mode, ReadMode::PrimaryFallback);
        assert_eq!(config.write_mode, WriteMode::AsyncReplication);
        assert!(config.primary_backend_name.is_none());

        match &config.backends[0] {
            BackendConfig::S3(s3_config) => {
                assert_eq!(s3_config.name, "aws-s3");
                assert_eq!(s3_config.region, "us-east-1");
                assert_eq!(s3_config.bucket, "my-bucket");
                assert_eq!(s3_config.force_path_style, false);
                assert!(s3_config.endpoint.is_none());
                assert!(s3_config.access_key_id.is_none());
                assert!(s3_config.secret_access_key.is_none());
            }
            _ => panic!("Expected S3 backend"),
        }
    }

    #[test]
    fn test_parse_s3_backend_full() {
        let json = r#"{
            "backends": [
                {
                    "type": "s3",
                    "name": "minio",
                    "region": "us-east-1",
                    "bucket": "my-bucket",
                    "endpoint": "http://localhost:9000",
                    "force_path_style": true,
                    "access_key_id": "minioadmin",
                    "secret_access_key": "minioadmin"
                }
            ],
            "readMode": "PRIMARY_ONLY",
            "writeMode": "MULTI_SYNC"
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.read_mode, ReadMode::PrimaryOnly);
        assert_eq!(config.write_mode, WriteMode::MultiSync);

        match &config.backends[0] {
            BackendConfig::S3(s3_config) => {
                assert_eq!(s3_config.name, "minio");
                assert_eq!(
                    s3_config.endpoint,
                    Some("http://localhost:9000".to_string())
                );
                assert_eq!(s3_config.force_path_style, true);
                assert_eq!(s3_config.access_key_id, Some("minioadmin".to_string()));
                assert_eq!(s3_config.secret_access_key, Some("minioadmin".to_string()));
            }
            _ => panic!("Expected S3 backend"),
        }
    }

    #[test]
    fn test_parse_memory_backend() {
        let json = r#"{
            "backends": [
                {
                    "type": "memory",
                    "name": "in-memory"
                }
            ],
            "readMode": "PRIMARY_FALLBACK",
            "writeMode": "ASYNC_REPLICATION"
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.backends.len(), 1);

        match &config.backends[0] {
            BackendConfig::Memory(mem_config) => {
                assert_eq!(mem_config.name, "in-memory");
            }
            _ => panic!("Expected Memory backend"),
        }
    }

    #[test]
    fn test_parse_multiple_backends() {
        let json = r#"{
            "backends": [
                {
                    "type": "s3",
                    "name": "aws",
                    "region": "us-east-1",
                    "bucket": "bucket1"
                },
                {
                    "type": "s3",
                    "name": "gcs",
                    "region": "us-central1",
                    "bucket": "bucket2"
                },
                {
                    "type": "memory",
                    "name": "cache"
                }
            ],
            "readMode": "BEST_EFFORT",
            "writeMode": "MULTI_SYNC",
            "primaryBackendName": "aws"
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.backends.len(), 3);
        assert_eq!(config.read_mode, ReadMode::BestEffort);
        assert_eq!(config.write_mode, WriteMode::MultiSync);
        assert_eq!(config.primary_backend_name, Some("aws".to_string()));

        assert!(matches!(config.backends[0], BackendConfig::S3(_)));
        assert!(matches!(config.backends[1], BackendConfig::S3(_)));
        assert!(matches!(config.backends[2], BackendConfig::Memory(_)));
    }

    #[test]
    fn test_parse_with_virtual_bucket() {
        let json = r#"{
            "virtualBucket": "my-virtual-bucket",
            "backends": [
                {
                    "type": "memory",
                    "name": "test"
                }
            ],
            "readMode": "PRIMARY_FALLBACK",
            "writeMode": "ASYNC_REPLICATION"
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.virtual_bucket, Some("my-virtual-bucket".to_string()));
    }

    #[test]
    fn test_parse_without_virtual_bucket() {
        let json = r#"{
            "backends": [
                {
                    "type": "memory",
                    "name": "test"
                }
            ],
            "readMode": "PRIMARY_FALLBACK",
            "writeMode": "ASYNC_REPLICATION"
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.virtual_bucket.is_none());
    }

    #[test]
    fn test_parse_invalid_json() {
        let json = "{ invalid json }";
        let result: Result<Config, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_required_field() {
        let json = r#"{
            "backends": [
                {
                    "type": "s3",
                    "name": "aws"
                }
            ]
        }"#;

        let result: Result<Config, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unknown_backend_type() {
        let json = r#"{
            "backends": [
                {
                    "type": "unknown",
                    "name": "test"
                }
            ]
        }"#;

        let result: Result<Config, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_file_valid() {
        let json = r#"{
            "backends": [
                {
                    "type": "memory",
                    "name": "test"
                }
            ],
            "readMode": "PRIMARY_FALLBACK",
            "writeMode": "ASYNC_REPLICATION"
        }"#;

        let mut temp_file = NamedTempFile::with_suffix(".json").unwrap();
        temp_file.write_all(json.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = Config::from_file(temp_file.path()).unwrap();
        assert_eq!(config.backends.len(), 1);
    }

    #[test]
    fn test_from_file_not_found() {
        let result = Config::from_file("/nonexistent/path/config.json");
        assert!(result.is_err());
    }

    #[test]
    fn test_from_file_invalid_json() {
        let mut temp_file = NamedTempFile::with_suffix(".json").unwrap();
        temp_file.write_all(b"invalid json").unwrap();
        temp_file.flush().unwrap();

        let result = Config::from_file(temp_file.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_serialize_s3_backend() {
        let config = Config {
            virtual_bucket: None,
            backends: vec![BackendConfig::S3(S3BackendConfig {
                name: "test".to_string(),
                region: "us-east-1".to_string(),
                bucket: "my-bucket".to_string(),
                endpoint: None,
                force_path_style: false,
                access_key_id: None,
                secret_access_key: None,
            })],
            read_mode: ReadMode::PrimaryFallback,
            write_mode: WriteMode::MultiSync,
            primary_backend_name: None,
            use_latency_based_primary_backend: None,
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.backends.len(), 1);
        assert_eq!(parsed.read_mode, ReadMode::PrimaryFallback);
        assert_eq!(parsed.write_mode, WriteMode::MultiSync);
        match &parsed.backends[0] {
            BackendConfig::S3(s3) => {
                assert_eq!(s3.name, "test");
                assert_eq!(s3.bucket, "my-bucket");
            }
            _ => panic!("Expected S3 backend"),
        }
    }

    #[test]
    fn test_serialize_skip_none_fields() {
        let config = Config {
            virtual_bucket: None,
            backends: vec![BackendConfig::S3(S3BackendConfig {
                name: "test".to_string(),
                region: "us-east-1".to_string(),
                bucket: "my-bucket".to_string(),
                endpoint: None,
                force_path_style: false,
                access_key_id: None,
                secret_access_key: None,
            })],
            read_mode: ReadMode::PrimaryFallback,
            write_mode: WriteMode::AsyncReplication,
            primary_backend_name: None,
            use_latency_based_primary_backend: None,
        };

        let json = serde_json::to_string(&config).unwrap();

        // Optional None fields should not appear in JSON
        assert!(!json.contains("virtualBucket"));
        assert!(!json.contains("endpoint"));
        assert!(!json.contains("access_key_id"));
        assert!(!json.contains("secret_access_key"));
        assert!(!json.contains("primaryBackendName"));
        assert!(!json.contains("useLatencyBasedPrimaryBackend"));
    }

    #[test]
    fn test_validate_unique_backend_names() {
        let json = r#"{
            "backends": [
                {
                    "type": "s3",
                    "name": "duplicate",
                    "region": "us-east-1",
                    "bucket": "bucket1"
                },
                {
                    "type": "s3",
                    "name": "duplicate",
                    "region": "us-west-2",
                    "bucket": "bucket2"
                }
            ],
            "readMode": "PRIMARY_FALLBACK",
            "writeMode": "ASYNC_REPLICATION"
        }"#;

        let result: Result<Config, _> = serde_json::from_str(json);
        assert!(result.is_ok());
        let config = result.unwrap();
        let validation_result = config.validate();
        assert!(validation_result.is_err());
        assert!(
            validation_result
                .unwrap_err()
                .to_string()
                .contains("Duplicate backend name")
        );
    }

    #[test]
    fn test_validate_primary_backend_exists() {
        let json = r#"{
            "backends": [
                {
                    "type": "s3",
                    "name": "backend1",
                    "region": "us-east-1",
                    "bucket": "bucket1"
                }
            ],
            "readMode": "PRIMARY_FALLBACK",
            "writeMode": "ASYNC_REPLICATION",
            "primaryBackendName": "nonexistent"
        }"#;

        let result: Result<Config, _> = serde_json::from_str(json);
        assert!(result.is_ok());
        let config = result.unwrap();
        let validation_result = config.validate();
        assert!(validation_result.is_err());
        assert!(
            validation_result
                .unwrap_err()
                .to_string()
                .contains("not found in backends list")
        );
    }

    #[test]
    fn test_validate_primary_backend_valid() {
        let json = r#"{
            "backends": [
                {
                    "type": "s3",
                    "name": "backend1",
                    "region": "us-east-1",
                    "bucket": "bucket1"
                },
                {
                    "type": "s3",
                    "name": "backend2",
                    "region": "us-west-2",
                    "bucket": "bucket2"
                }
            ],
            "readMode": "PRIMARY_FALLBACK",
            "writeMode": "ASYNC_REPLICATION",
            "primaryBackendName": "backend1"
        }"#;

        let result: Result<Config, _> = serde_json::from_str(json);
        assert!(result.is_ok());
        let config = result.unwrap();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_backend_config_name() {
        let s3_backend = BackendConfig::S3(S3BackendConfig {
            name: "s3-test".to_string(),
            region: "us-east-1".to_string(),
            bucket: "bucket".to_string(),
            endpoint: None,
            force_path_style: false,
            access_key_id: None,
            secret_access_key: None,
        });
        assert_eq!(s3_backend.name(), "s3-test");

        let mem_backend = BackendConfig::Memory(MemoryBackendConfig {
            name: "mem-test".to_string(),
        });
        assert_eq!(mem_backend.name(), "mem-test");
    }

    #[test]
    fn test_validate_mutually_exclusive_primary_selection() {
        let json = r#"{
            "backends": [
                {
                    "type": "s3",
                    "name": "backend1",
                    "region": "us-east-1",
                    "bucket": "bucket1"
                }
            ],
            "readMode": "PRIMARY_FALLBACK",
            "writeMode": "ASYNC_REPLICATION",
            "primaryBackendName": "backend1",
            "useLatencyBasedPrimaryBackend": true
        }"#;

        let result: Result<Config, _> = serde_json::from_str(json);
        assert!(result.is_ok());
        let config = result.unwrap();
        let validation_result = config.validate();
        assert!(validation_result.is_err());
        assert!(
            validation_result
                .unwrap_err()
                .to_string()
                .contains("Cannot specify both")
        );
    }

    #[test]
    fn test_parse_yaml_minimal() {
        let yaml = r#"
backends:
  - type: s3
    name: aws-s3
    region: us-east-1
    bucket: my-bucket
readMode: PRIMARY_FALLBACK
writeMode: ASYNC_REPLICATION
"#;

        let config: Config = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.backends.len(), 1);
        assert_eq!(config.read_mode, ReadMode::PrimaryFallback);
        assert_eq!(config.write_mode, WriteMode::AsyncReplication);
        assert!(config.primary_backend_name.is_none());

        match &config.backends[0] {
            BackendConfig::S3(s3_config) => {
                assert_eq!(s3_config.name, "aws-s3");
                assert_eq!(s3_config.region, "us-east-1");
                assert_eq!(s3_config.bucket, "my-bucket");
            }
            _ => panic!("Expected S3 backend"),
        }
    }

    #[test]
    fn test_parse_yaml_full() {
        let yaml = r#"
virtualBucket: my-virtual-bucket
backends:
  - type: s3
    name: minio
    region: us-east-1
    bucket: my-bucket
    endpoint: "http://localhost:9000"
    force_path_style: true
    access_key_id: minioadmin
    secret_access_key: minioadmin
readMode: PRIMARY_ONLY
writeMode: MULTI_SYNC
primaryBackendName: minio
"#;

        let config: Config = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.virtual_bucket, Some("my-virtual-bucket".to_string()));
        assert_eq!(config.read_mode, ReadMode::PrimaryOnly);
        assert_eq!(config.write_mode, WriteMode::MultiSync);
        assert_eq!(config.primary_backend_name, Some("minio".to_string()));

        match &config.backends[0] {
            BackendConfig::S3(s3_config) => {
                assert_eq!(s3_config.name, "minio");
                assert_eq!(
                    s3_config.endpoint,
                    Some("http://localhost:9000".to_string())
                );
                assert!(s3_config.force_path_style);
                assert_eq!(s3_config.access_key_id, Some("minioadmin".to_string()));
                assert_eq!(s3_config.secret_access_key, Some("minioadmin".to_string()));
            }
            _ => panic!("Expected S3 backend"),
        }
    }

    #[test]
    fn test_detect_format_json() {
        let result = Config::detect_format(Path::new("config.json"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ConfigFormat::Json);
    }

    #[test]
    fn test_detect_format_yaml() {
        let result = Config::detect_format(Path::new("config.yaml"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ConfigFormat::Yaml);
    }

    #[test]
    fn test_detect_format_yml() {
        let result = Config::detect_format(Path::new("config.yml"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ConfigFormat::Yaml);
    }

    #[test]
    fn test_detect_format_case_insensitive() {
        assert_eq!(
            Config::detect_format(Path::new("config.JSON")).unwrap(),
            ConfigFormat::Json
        );
        assert_eq!(
            Config::detect_format(Path::new("config.YAML")).unwrap(),
            ConfigFormat::Yaml
        );
        assert_eq!(
            Config::detect_format(Path::new("config.YML")).unwrap(),
            ConfigFormat::Yaml
        );
    }

    #[test]
    fn test_detect_format_unsupported_extension() {
        let result = Config::detect_format(Path::new("config.toml"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported"));
    }

    #[test]
    fn test_detect_format_no_extension() {
        let result = Config::detect_format(Path::new("config"));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must have an extension"));
    }

    #[test]
    fn test_from_file_yaml() {
        let yaml = r#"
backends:
  - type: memory
    name: test
readMode: PRIMARY_FALLBACK
writeMode: ASYNC_REPLICATION
"#;

        let mut temp_file = NamedTempFile::with_suffix(".yaml").unwrap();
        temp_file.write_all(yaml.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = Config::from_file(temp_file.path()).unwrap();
        assert_eq!(config.backends.len(), 1);
        match &config.backends[0] {
            BackendConfig::Memory(mem) => {
                assert_eq!(mem.name, "test");
            }
            _ => panic!("Expected Memory backend"),
        }
    }

    #[test]
    fn test_from_file_yml() {
        let yaml = r#"
backends:
  - type: memory
    name: test
readMode: PRIMARY_FALLBACK
writeMode: ASYNC_REPLICATION
"#;

        let mut temp_file = NamedTempFile::with_suffix(".yml").unwrap();
        temp_file.write_all(yaml.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = Config::from_file(temp_file.path()).unwrap();
        assert_eq!(config.backends.len(), 1);
    }

    #[test]
    fn test_json_yaml_equivalence() {
        let json = r#"{
            "virtualBucket": "test-bucket",
            "backends": [
                {
                    "type": "s3",
                    "name": "aws",
                    "region": "us-east-1",
                    "bucket": "my-bucket",
                    "endpoint": "http://localhost:9000",
                    "force_path_style": true
                }
            ],
            "readMode": "PRIMARY_FALLBACK",
            "writeMode": "ASYNC_REPLICATION",
            "primaryBackendName": "aws"
        }"#;

        let yaml = r#"
virtualBucket: test-bucket
backends:
  - type: s3
    name: aws
    region: us-east-1
    bucket: my-bucket
    endpoint: "http://localhost:9000"
    force_path_style: true
readMode: PRIMARY_FALLBACK
writeMode: ASYNC_REPLICATION
primaryBackendName: aws
"#;

        let json_config: Config = serde_json::from_str(json).unwrap();
        let yaml_config: Config = serde_yml::from_str(yaml).unwrap();

        assert_eq!(json_config.virtual_bucket, yaml_config.virtual_bucket);
        assert_eq!(json_config.read_mode, yaml_config.read_mode);
        assert_eq!(json_config.write_mode, yaml_config.write_mode);
        assert_eq!(
            json_config.primary_backend_name,
            yaml_config.primary_backend_name
        );
        assert_eq!(json_config.backends.len(), yaml_config.backends.len());

        match (&json_config.backends[0], &yaml_config.backends[0]) {
            (BackendConfig::S3(json_s3), BackendConfig::S3(yaml_s3)) => {
                assert_eq!(json_s3.name, yaml_s3.name);
                assert_eq!(json_s3.region, yaml_s3.region);
                assert_eq!(json_s3.bucket, yaml_s3.bucket);
                assert_eq!(json_s3.endpoint, yaml_s3.endpoint);
                assert_eq!(json_s3.force_path_style, yaml_s3.force_path_style);
            }
            _ => panic!("Expected S3 backends"),
        }
    }
}
