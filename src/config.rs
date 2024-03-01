use crate::error::IntegrityVerificationError;
use serde_derive::Deserialize;

const fn default_test_retries() -> u64 {
    20
}

#[derive(Deserialize, Debug)]
pub struct IntegrityVerificationConfig {
    pub reference_host: String,
    pub testing_host: String,
    pub rpc_endpoint: String,
    pub testing_file_path: String,
    #[serde(default = "default_test_retries")]
    pub test_retries: u64,
    #[serde(default)]
    pub log_differences: bool,
    #[serde(default)]
    pub difference_filter_regexes: Vec<String>,
}

pub fn setup_config(path: &str) -> Result<IntegrityVerificationConfig, IntegrityVerificationError> {
    let data = std::fs::read_to_string(path)?;
    let c: IntegrityVerificationConfig = serde_json::from_str(data.as_str())?;
    validate_config(&c)?;

    Ok(c)
}

fn validate_config(config: &IntegrityVerificationConfig) -> Result<(), IntegrityVerificationError> {
    if config.test_retries < 1 {
        return Err(IntegrityVerificationError::ValidateConfig(
            "test_retries".to_string(),
        ));
    }
    Ok(())
}
