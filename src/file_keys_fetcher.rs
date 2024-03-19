use crate::diff_checker::{
    GET_ASSET_BY_AUTHORITY_METHOD, GET_ASSET_BY_CREATOR_METHOD, GET_ASSET_BY_GROUP_METHOD,
    GET_ASSET_BY_OWNER_METHOD, GET_ASSET_METHOD, GET_ASSET_PROOF_METHOD, GET_SIGNATURES_FOR_ASSET,
    GET_TOKEN_ACCOUNTS_BY_MINT, GET_TOKEN_ACCOUNTS_BY_OWNER, GET_TOKEN_ACCOUNTS_BY_OWNER_AND_MINT,
};
use crate::interfaces::IntegrityVerificationKeysFetcher;
use async_trait::async_trait;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Clone)]
pub struct FileKeysFetcher {
    pub keys_map: HashMap<String, Vec<String>>,
    rnd: StdRng,
}

impl FileKeysFetcher {
    pub async fn new(file_path: &str) -> Result<Self, String> {
        let file = File::open(file_path).await.map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        let mut keys_map = HashMap::new();
        let mut current_key = None;

        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            if line.ends_with(':') {
                current_key = Some(line.trim_end_matches(':').to_string());
            } else if let Some(key) = &current_key {
                if !line.is_empty() {
                    for pubkey in line.split(',').map(String::from) {
                        if pubkey.is_empty() {
                            continue;
                        }
                        keys_map
                            .entry(key.clone())
                            .or_insert_with(Vec::new)
                            .push(pubkey);
                    }
                }
            }
        }

        let rnd = StdRng::from_entropy();

        Ok(FileKeysFetcher { keys_map, rnd })
    }
    fn read_keys(&self, method_name: &str) -> Result<Vec<String>, String> {
        Ok(self.keys_map.get(method_name).cloned().unwrap_or_default())
    }

    pub fn get_random_command(&mut self) -> (String, String) {
        let commands: Vec<&String> = self.keys_map.keys().collect();

        let command_ind = self.rnd.gen_range(0..commands.len());

        let command_args_len = self.keys_map.get(commands[command_ind]).unwrap().len();

        let arg_ind = self.rnd.gen_range(0..command_args_len);

        let arg = self.keys_map.get(commands[command_ind]).unwrap()[arg_ind].clone();

        (commands[command_ind].clone(), arg)
    }
}
#[async_trait]
impl IntegrityVerificationKeysFetcher for FileKeysFetcher {
    async fn get_verification_required_owners_keys(&self) -> Result<Vec<String>, String> {
        self.read_keys(GET_ASSET_BY_OWNER_METHOD)
    }

    async fn get_verification_required_creators_keys(&self) -> Result<Vec<String>, String> {
        self.read_keys(GET_ASSET_BY_CREATOR_METHOD)
    }

    async fn get_verification_required_authorities_keys(&self) -> Result<Vec<String>, String> {
        self.read_keys(GET_ASSET_BY_AUTHORITY_METHOD)
    }

    async fn get_verification_required_groups_keys(&self) -> Result<Vec<String>, String> {
        self.read_keys(GET_ASSET_BY_GROUP_METHOD)
    }

    async fn get_verification_required_assets_keys(&self) -> Result<Vec<String>, String> {
        self.read_keys(GET_ASSET_METHOD)
    }

    async fn get_verification_required_assets_proof_keys(&self) -> Result<Vec<String>, String> {
        self.read_keys(GET_ASSET_PROOF_METHOD)
    }

    async fn get_verification_required_tokens_by_owner(&self) -> Result<Vec<String>, String> {
        self.read_keys(GET_TOKEN_ACCOUNTS_BY_OWNER)
    }

    async fn get_verification_required_tokens_by_mint(&self) -> Result<Vec<String>, String> {
        self.read_keys(GET_TOKEN_ACCOUNTS_BY_MINT)
    }

    async fn get_verification_required_tokens_by_owner_and_mint(
        &self,
    ) -> Result<Vec<(String, String)>, String> {
        let sets = self.read_keys(GET_TOKEN_ACCOUNTS_BY_OWNER_AND_MINT)?;

        let mut pairs = Vec::new();

        for pair in sets.iter() {
            let owner_mint: Vec<String> = pair
                .trim_matches(|c| c == '(' || c == ')')
                .split(';')
                .map(String::from)
                .collect();

            pairs.push((owner_mint[0].clone(), owner_mint[1].clone()));
        }

        Ok(pairs)
    }

    async fn get_verification_required_signatures_for_asset(&self) -> Result<Vec<String>, String> {
        self.read_keys(GET_SIGNATURES_FOR_ASSET)
    }
}
