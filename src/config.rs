use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_bucket: Option<String>,
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

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
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
            ]
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.backends.len(), 1);

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
            ]
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
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
            ]
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
            ]
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.backends.len(), 3);

        assert!(matches!(config.backends[0], BackendConfig::S3(_)));
        assert!(matches!(config.backends[1], BackendConfig::S3(_)));
        assert!(matches!(config.backends[2], BackendConfig::Memory(_)));
    }

    #[test]
    fn test_parse_with_virtual_bucket() {
        let json = r#"{
            "virtual_bucket": "my-virtual-bucket",
            "backends": [
                {
                    "type": "memory",
                    "name": "test"
                }
            ]
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
            ]
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
            ]
        }"#;

        let mut temp_file = NamedTempFile::new().unwrap();
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
        let mut temp_file = NamedTempFile::new().unwrap();
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
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.backends.len(), 1);
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
        };

        let json = serde_json::to_string(&config).unwrap();

        // Optional None fields should not appear in JSON
        assert!(!json.contains("virtual_bucket"));
        assert!(!json.contains("endpoint"));
        assert!(!json.contains("access_key_id"));
        assert!(!json.contains("secret_access_key"));
    }
}
