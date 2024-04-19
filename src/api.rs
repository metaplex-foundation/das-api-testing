use crate::error::IntegrityVerificationError;
use reqwest::Client;

#[derive(Debug)]
pub struct IntegrityVerificationApi {
    client: Client,
}

impl IntegrityVerificationApi {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    pub async fn make_request(
        &self,
        url: &str,
        body: &str,
    ) -> Result<serde_json::Value, IntegrityVerificationError> {
        let resp = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .body(body.to_owned())
            .send()
            .await?;

        let code = resp.status();

        if code != reqwest::StatusCode::OK {
            return Err(IntegrityVerificationError::ResponseStatusCode(
                code.as_u16(),
            ));
        }

        let resp_body = resp.text().await?;

        Ok(serde_json::from_str(resp_body.as_str())?)
    }
}
