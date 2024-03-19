use async_trait::async_trait;
use mockall::automock;

#[automock]
#[async_trait]
pub trait IntegrityVerificationKeysFetcher {
    async fn get_verification_required_owners_keys(&self) -> Result<Vec<String>, String>;
    async fn get_verification_required_creators_keys(&self) -> Result<Vec<String>, String>;
    async fn get_verification_required_authorities_keys(&self) -> Result<Vec<String>, String>;
    async fn get_verification_required_groups_keys(&self) -> Result<Vec<String>, String>;
    async fn get_verification_required_assets_keys(&self) -> Result<Vec<String>, String>;
    async fn get_verification_required_assets_proof_keys(&self) -> Result<Vec<String>, String>;
    async fn get_verification_required_tokens_by_owner(&self) -> Result<Vec<String>, String>;
    async fn get_verification_required_tokens_by_mint(&self) -> Result<Vec<String>, String>;
    async fn get_verification_required_tokens_by_owner_and_mint(
        &self,
    ) -> Result<Vec<(String, String)>, String>;
    async fn get_verification_required_signatures_for_asset(&self) -> Result<Vec<String>, String>;
}
