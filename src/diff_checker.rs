use crate::api::IntegrityVerificationApi;
use crate::config::IntegrityVerificationConfig;
use crate::error::IntegrityVerificationError;
use crate::interfaces::IntegrityVerificationKeysFetcher;
use crate::params_generation::{
    generate_get_asset_params, generate_get_asset_proof_params,
    generate_get_assets_by_authority_params, generate_get_assets_by_creator_params,
    generate_get_assets_by_group_params, generate_get_assets_by_owner_params,
    generate_get_signatures_for_asset, generate_get_token_accounts,
};
use crate::requests::Body;
use crate::{_check_proof, check_proof};
use anchor_lang::AnchorDeserialize;
use assert_json_diff::{assert_json_matches_no_panic, CompareMode, Config};
use regex::Regex;
use serde_json::{json, Value};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::pubkey::Pubkey;
use solana_sdk::commitment_config::{CommitmentConfig, CommitmentLevel};
use spl_account_compression::canopy::fill_in_proof_from_canopy;
use spl_account_compression::state::{
    merkle_tree_get_size, ConcurrentMerkleTreeHeader, CONCURRENT_MERKLE_TREE_HEADER_SIZE_V1,
};
use spl_account_compression::zero_copy::ZeroCopy;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::error;
use tracing::log::info;

pub const GET_ASSET_METHOD: &str = "getAsset";
pub const GET_ASSET_PROOF_METHOD: &str = "getAssetProof";
pub const GET_ASSET_BY_OWNER_METHOD: &str = "getAssetsByOwner";
pub const GET_ASSET_BY_AUTHORITY_METHOD: &str = "getAssetsByAuthority";
pub const GET_ASSET_BY_GROUP_METHOD: &str = "getAssetsByGroup";
pub const GET_ASSET_BY_CREATOR_METHOD: &str = "getAssetsByCreator";
pub const GET_TOKEN_ACCOUNTS_BY_OWNER: &str = "getTokenAccountsByOwner";
pub const GET_TOKEN_ACCOUNTS_BY_MINT: &str = "getTokenAccountsByMint";
pub const GET_TOKEN_ACCOUNTS_BY_OWNER_AND_MINT: &str = "getTokenAccountsByOwnerAndMint";
pub const GET_SIGNATURES_FOR_ASSET: &str = "getSignaturesForAsset";

const REQUESTS_INTERVAL_MILLIS: u64 = 1500;

#[derive(Default)]
struct TestingResult {
    total_tests: u64,
    failed_tests: u64,
}

struct TestingResults(Mutex<HashMap<String, TestingResult>>);
impl TestingResults {
    fn new() -> Self {
        TestingResults(Mutex::new(HashMap::new()))
    }
    async fn inc_total_tests(&self, method: &str) {
        self.modify_result(method, |res| res.total_tests += 1).await;
    }

    async fn inc_failed_tests(&self, method: &str) {
        self.modify_result(method, |res| res.failed_tests += 1)
            .await;
    }

    async fn modify_result<F>(&self, method: &str, mut f: F)
    where
        F: FnMut(&mut TestingResult),
    {
        let mut map = self.0.lock().await;
        let entry = map
            .entry(method.to_string())
            .or_insert_with(TestingResult::default);
        f(entry);
    }
}

#[derive(Default)]
struct DiffWithResponses {
    diff: Option<String>,
    testing_response: Value,
}

pub struct DiffChecker<T>
where
    T: IntegrityVerificationKeysFetcher + Send + Sync,
{
    reference_host: String,
    testing_host: String,
    api: IntegrityVerificationApi,
    keys_fetcher: T,
    rpc_client: RpcClient,
    regexes: Vec<Regex>,
    test_retries: u64,
    test_results: TestingResults,
    log_differences: bool,
}

impl<T> DiffChecker<T>
where
    T: IntegrityVerificationKeysFetcher + Send + Sync,
{
    pub async fn new(
        config: &IntegrityVerificationConfig,
        keys_fetcher: T,
    ) -> Result<Self, IntegrityVerificationError> {
        // Regular expressions, that purposed to filter out some difference between
        // testing and reference hosts that you already know about
        // Using unwraps is safe, if we pass correct patterns into Regex::new
        let regexes = config
            .difference_filter_regexes
            .iter()
            .map(|r| {
                Regex::new(r).map_err(|e| IntegrityVerificationError::InvalidRegex(e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            rpc_client: RpcClient::new(config.rpc_endpoint.clone()),
            reference_host: config.reference_host.clone(),
            testing_host: config.testing_host.clone(),
            api: IntegrityVerificationApi::new(),
            keys_fetcher,
            regexes,
            test_retries: config.test_retries,
            test_results: TestingResults::new(),
            log_differences: config.log_differences,
        })
    }

    pub async fn show_results(&self) {
        for (method, result) in self.test_results.0.lock().await.iter() {
            info!(
                "RESULTS OF {} METHOD TEST: TESTED PUBKEYS TOTAL: {}, FAILED TESTS: {}",
                method, result.total_tests, result.failed_tests
            );
        }
    }
}

impl<T> DiffChecker<T>
where
    T: IntegrityVerificationKeysFetcher + Send + Sync,
{
    pub fn compare_responses(
        &self,
        reference_response: &Value,
        testing_response: &Value,
    ) -> Option<String> {
        if let Err(diff) = assert_json_matches_no_panic(
            &reference_response,
            &testing_response,
            Config::new(CompareMode::Strict),
        ) {
            let diff = self
                .regexes
                .iter()
                .fold(diff, |acc, re| re.replace_all(&acc, "").to_string());
            if diff.is_empty() {
                return None;
            }

            return Some(diff);
        }

        None
    }

    async fn check_request(&self, req: &Body) -> DiffWithResponses {
        let request = json!(req).to_string();
        let reference_response_fut = self.api.make_request(&self.reference_host, &request);
        let testing_response_fut = self.api.make_request(&self.testing_host, &request);
        let (reference_response, testing_response) =
            tokio::join!(reference_response_fut, testing_response_fut);

        let reference_response = match reference_response {
            Ok(reference_response) => reference_response,
            Err(e) => {
                error!("Reference host network error: {}", e);
                return DiffWithResponses::default();
            }
        };
        let testing_response = match testing_response {
            Ok(testing_response) => testing_response,
            Err(e) => {
                error!("Testing host network error: {}", e);
                return DiffWithResponses::default();
            }
        };

        DiffWithResponses {
            diff: self.compare_responses(&reference_response, &testing_response),
            testing_response,
        }
    }

    async fn check_requests(&self, requests: Vec<Body>) {
        for req in requests.iter() {
            self.test_results.inc_total_tests(&req.method).await;
            let mut diff_with_responses = DiffWithResponses::default();
            for _ in 0..self.test_retries {
                diff_with_responses = self.check_request(req).await;
                if diff_with_responses.diff.is_none() {
                    break;
                }
                // Prevent rate-limit errors
                tokio::time::sleep(Duration::from_millis(REQUESTS_INTERVAL_MILLIS)).await;
            }

            let mut test_failed = false;
            if let Some(diff) = diff_with_responses.diff {
                test_failed = true;
                if self.log_differences {
                    error!(
                        "{}: mismatch responses: req: {:#?}, diff: {}",
                        req.method, req, diff
                    );
                }
            }

            if req.method == GET_ASSET_PROOF_METHOD {
                let asset_id = req.params["id"].as_str().unwrap_or_default();
                test_failed = match self
                    .check_proof_valid(asset_id, diff_with_responses.testing_response)
                    .await
                {
                    Ok(proof_valid) => {
                        if !proof_valid {
                            error!("Invalid proof for {} asset", asset_id)
                        };
                        !proof_valid
                    }
                    Err(e) => {
                        error!("Check proof valid: {}", e);
                        test_failed
                    }
                };
            }
            if test_failed {
                self.test_results.inc_failed_tests(&req.method).await;
            }

            // Prevent rate-limit errors
            tokio::time::sleep(Duration::from_millis(REQUESTS_INTERVAL_MILLIS)).await;
        }
    }

    pub async fn check_get_asset(&self) -> Result<(), IntegrityVerificationError> {
        let verification_required_keys = self
            .keys_fetcher
            .get_verification_required_assets_keys()
            .await
            .map_err(IntegrityVerificationError::FetchKeys)?;

        let requests = verification_required_keys
            .into_iter()
            .map(|key| Body::new(GET_ASSET_METHOD, json!(generate_get_asset_params(key))))
            .collect::<Vec<_>>();

        self.check_requests(requests).await;

        Ok(())
    }

    pub async fn check_get_asset_proof(&self) -> Result<(), IntegrityVerificationError> {
        let verification_required_keys = self
            .keys_fetcher
            .get_verification_required_assets_proof_keys()
            .await
            .map_err(IntegrityVerificationError::FetchKeys)?;

        let requests = verification_required_keys
            .into_iter()
            .map(|key| {
                Body::new(
                    GET_ASSET_PROOF_METHOD,
                    json!(generate_get_asset_proof_params(key)),
                )
            })
            .collect::<Vec<_>>();

        self.check_requests(requests).await;

        Ok(())
    }

    pub async fn check_get_asset_by_authority(&self) -> Result<(), IntegrityVerificationError> {
        let verification_required_keys = self
            .keys_fetcher
            .get_verification_required_authorities_keys()
            .await
            .map_err(IntegrityVerificationError::FetchKeys)?;

        let requests = verification_required_keys
            .into_iter()
            .map(|key| {
                Body::new(
                    GET_ASSET_BY_AUTHORITY_METHOD,
                    json!(generate_get_assets_by_authority_params(key, None, None)),
                )
            })
            .collect::<Vec<_>>();

        self.check_requests(requests).await;

        Ok(())
    }

    pub async fn check_get_asset_by_owner(&self) -> Result<(), IntegrityVerificationError> {
        let verification_required_keys = self
            .keys_fetcher
            .get_verification_required_owners_keys()
            .await
            .map_err(IntegrityVerificationError::FetchKeys)?;

        let requests = verification_required_keys
            .into_iter()
            .map(|key| {
                Body::new(
                    GET_ASSET_BY_OWNER_METHOD,
                    json!(generate_get_assets_by_owner_params(key, None, None)),
                )
            })
            .collect::<Vec<_>>();

        self.check_requests(requests).await;

        Ok(())
    }

    pub async fn check_get_asset_by_group(&self) -> Result<(), IntegrityVerificationError> {
        let verification_required_keys = self
            .keys_fetcher
            .get_verification_required_groups_keys()
            .await
            .map_err(IntegrityVerificationError::FetchKeys)?;

        let requests = verification_required_keys
            .into_iter()
            .map(|key| {
                Body::new(
                    GET_ASSET_BY_GROUP_METHOD,
                    json!(generate_get_assets_by_group_params(key, None, None)),
                )
            })
            .collect::<Vec<_>>();

        self.check_requests(requests).await;

        Ok(())
    }

    pub async fn check_get_asset_by_creator(&self) -> Result<(), IntegrityVerificationError> {
        let verification_required_keys = self
            .keys_fetcher
            .get_verification_required_creators_keys()
            .await
            .map_err(IntegrityVerificationError::FetchKeys)?;

        let requests = verification_required_keys
            .into_iter()
            .map(|key| {
                Body::new(
                    GET_ASSET_BY_CREATOR_METHOD,
                    json!(generate_get_assets_by_creator_params(key, None, None)),
                )
            })
            .collect::<Vec<_>>();

        self.check_requests(requests).await;

        Ok(())
    }

    async fn check_proof_valid(
        &self,
        asset_id: &str,
        response: Value,
    ) -> Result<bool, IntegrityVerificationError> {
        let tree_id = response["result"]["tree_id"].as_str().ok_or(
            IntegrityVerificationError::CannotGetResponseField("tree_id".to_string()),
        )?;
        let leaf = Pubkey::from_str(response["result"]["leaf"].as_str().ok_or(
            IntegrityVerificationError::CannotGetResponseField("leaf".to_string()),
        )?)?
        .to_bytes();

        let get_asset_req = json!(&Body::new(
            GET_ASSET_METHOD,
            json!(generate_get_asset_params(asset_id.to_string()))
        ))
        .to_string();
        let get_asset_fut = self.api.make_request(&self.reference_host, &get_asset_req);
        let tree_id_pk = Pubkey::from_str(tree_id)?;
        let get_account_data_fut = self.rpc_client.get_account_with_commitment(
            &tree_id_pk,
            CommitmentConfig {
                commitment: CommitmentLevel::Processed,
            },
        );
        let (get_asset, account_data) = tokio::join!(get_asset_fut, get_account_data_fut);
        let get_asset = get_asset?;
        let leaf_index = get_asset["result"]["compression"]["leaf_id"]
            .as_u64()
            .ok_or(IntegrityVerificationError::CannotGetResponseField(
                "leaf_id".to_string(),
            ))? as u32;
        let mut tree_acc_info = account_data?
            .value
            .ok_or(IntegrityVerificationError::NullAssetAccount(
                tree_id_pk.to_string(),
            ))?
            .data;

        let (header_bytes, rest) =
            tree_acc_info.split_at_mut(CONCURRENT_MERKLE_TREE_HEADER_SIZE_V1);
        let header = ConcurrentMerkleTreeHeader::try_from_slice(header_bytes)?;
        let merkle_tree_size = merkle_tree_get_size(&header)?;
        let (tree_bytes, canopy_bytes) = rest.split_at_mut(merkle_tree_size);

        let mut initial_proofs = response["result"]["proof"]
            .as_array()
            .ok_or(IntegrityVerificationError::CannotGetResponseField(
                "proof".to_string(),
            ))?
            .iter()
            .filter_map(|proof| {
                proof
                    .as_str()
                    .and_then(|v| Pubkey::from_str(v).ok().map(|p| p.to_bytes()))
            })
            .collect::<Vec<_>>();
        fill_in_proof_from_canopy(
            canopy_bytes,
            header.get_max_depth(),
            leaf_index,
            &mut initial_proofs,
        )?;

        check_proof!(&header, &tree_bytes, initial_proofs, leaf, leaf_index)
    }

    pub async fn check_get_token_accounts_by_owner(
        &self,
    ) -> Result<(), IntegrityVerificationError> {
        let verification_required_keys = self
            .keys_fetcher
            .get_verification_required_tokens_by_owner()
            .await
            .map_err(IntegrityVerificationError::FetchKeys)?;

        let requests = verification_required_keys
            .into_iter()
            .map(|owner| {
                Body::new(
                    GET_TOKEN_ACCOUNTS_BY_OWNER,
                    json!(generate_get_token_accounts(Some(owner), None)),
                )
            })
            .collect::<Vec<_>>();

        self.check_requests(requests).await;

        Ok(())
    }

    pub async fn check_get_token_accounts_by_mint(&self) -> Result<(), IntegrityVerificationError> {
        let verification_required_keys = self
            .keys_fetcher
            .get_verification_required_tokens_by_mint()
            .await
            .map_err(IntegrityVerificationError::FetchKeys)?;

        let requests = verification_required_keys
            .into_iter()
            .map(|mint| {
                Body::new(
                    GET_TOKEN_ACCOUNTS_BY_MINT,
                    json!(generate_get_token_accounts(None, Some(mint))),
                )
            })
            .collect::<Vec<_>>();

        self.check_requests(requests).await;

        Ok(())
    }

    pub async fn check_get_token_accounts_by_owner_and_mint(
        &self,
    ) -> Result<(), IntegrityVerificationError> {
        let verification_required_keys = self
            .keys_fetcher
            .get_verification_required_tokens_by_owner_and_mint()
            .await
            .map_err(IntegrityVerificationError::FetchKeys)?;

        let requests = verification_required_keys
            .into_iter()
            .map(|pair| {
                Body::new(
                    GET_TOKEN_ACCOUNTS_BY_OWNER_AND_MINT,
                    json!(generate_get_token_accounts(Some(pair.0), Some(pair.1))),
                )
            })
            .collect::<Vec<_>>();

        self.check_requests(requests).await;

        Ok(())
    }

    pub async fn check_get_signatures_for_asset(&self) -> Result<(), IntegrityVerificationError> {
        let verification_required_keys = self
            .keys_fetcher
            .get_verification_required_signatures_for_asset()
            .await
            .map_err(IntegrityVerificationError::FetchKeys)?;

        let requests = verification_required_keys
            .into_iter()
            .map(|asset| {
                Body::new(
                    GET_SIGNATURES_FOR_ASSET,
                    json!(generate_get_signatures_for_asset(asset)),
                )
            })
            .collect::<Vec<_>>();

        self.check_requests(requests).await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use assert_json_diff::{assert_json_matches_no_panic, CompareMode, Config};
    use regex::Regex;
    use serde_json::json;

    #[tokio::test]
    async fn test_regex() {
        let reference_response = json!({
            "jsonrpc": "2.0",
            "result": {
                    "files": [
                        {
                            "uri": "https://assets.pinit.io/3Qru1Gjz9SFd4nESynRQytL65nXNcQGwc1eVbZz24ijG/ZyFU9Lt94Rb57y2hZpAssPCRQU6qXoWzkPhd6bEHKep/731.jpeg",
                            "mime": "image/jpeg"
                        }
                    ],
                    "metadata": {
                        "description": "GK #731 - Generated and deployed on LaunchMyNFT.",
                        "name": "NFT #731",
                        "symbol": "SYM",
                        "token_standard": "NonFungible"
                    },
                },
            "id": 0
        });

        let testing_response1 = json!({
        "jsonrpc": "2.0",
        "result": {
                "files": [
                    {
                        "uri": "https://assets.pinit.io/3Qru1Gjz9SFd4nESynRQytL65nXNcQGwc1eVbZz24ijG/ZyFU9Lt94Rb57y2hZpAssPCRQU6qXoWzkPhd6bEHKep/731.jpeg",
                        "mime": "image/jpeg"
                    }
                ],
                "metadata": {
                    "description": "GK #731 - Generated and deployed on LaunchMyNFT.",
                    "name": "NFT #731",
                    "symbol": "SYM",
                },
            },
            "id": 0
        });

        let res = assert_json_matches_no_panic(
            &reference_response,
            &testing_response1,
            Config::new(CompareMode::Strict),
        )
        .err()
        .unwrap();

        let re1 = Regex::new(r#"json atom at path \".*?\.token_standard\" is missing from rhs\n*"#)
            .unwrap();
        let res = re1.replace_all(&res, "").to_string();

        assert_eq!(0, res.len());

        let testing_response2 = json!({
        "jsonrpc": "2.0",
        "result": {
                "files": [
                    {
                        "uri": "https://assets.pinit.io/3Qru1Gjz9SFd4nESynRQytL65nXNcQGwc1eVbZz24ijG/ZyFU9Lt94Rb57y2hZpAssPCRQU6qXoWzkPhd6bEHKep/731.jpeg",
                        "mime": "image/jpeg"
                    }
                ],
                "mutable": false,
                "metadata": {
                    "description": "GK #731 - Generated and deployed on LaunchMyNFT.",
                    "name": "NFT #731",
                    "symbol": "SYM",
                },
            },
            "id": 0
        });

        let res = assert_json_matches_no_panic(
            &reference_response,
            &testing_response2,
            Config::new(CompareMode::Strict),
        )
        .err()
        .unwrap();

        let res = re1.replace_all(&res, "").to_string();

        assert_eq!(
            "json atom at path \".result.mutable\" is missing from lhs",
            res.trim()
        );
    }
}
