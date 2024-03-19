use std::{collections::HashMap, fmt, sync::Arc};

use crate::{
    api::IntegrityVerificationApi,
    diff_checker::{
        GET_ASSET_BY_AUTHORITY_METHOD, GET_ASSET_BY_CREATOR_METHOD, GET_ASSET_BY_GROUP_METHOD,
        GET_ASSET_BY_OWNER_METHOD, GET_ASSET_METHOD, GET_ASSET_PROOF_METHOD,
        GET_SIGNATURES_FOR_ASSET, GET_TOKEN_ACCOUNTS, GET_TOKEN_ACCOUNTS_BY_MINT,
        GET_TOKEN_ACCOUNTS_BY_OWNER, GET_TOKEN_ACCOUNTS_BY_OWNER_AND_MINT,
    },
    error::IntegrityVerificationError,
    file_keys_fetcher::FileKeysFetcher,
    graceful_stop,
    params_generation::{
        generate_get_asset_params, generate_get_asset_proof_params,
        generate_get_assets_by_authority_params, generate_get_assets_by_creator_params,
        generate_get_assets_by_group_params, generate_get_assets_by_owner_params,
        generate_get_signatures_for_asset, generate_get_token_accounts,
    },
    requests::Body,
};
use serde_json::json;
use tokio::{
    sync::{
        watch::{self, Receiver},
        Mutex,
    },
    task::JoinSet,
};
use tracing::{debug, info};

pub enum Commands {
    Init,
    Start(Vec<u32>),
    Stop(Vec<u32>),
}

pub struct Stats {
    successful_requests: u64,
    failed_requests: u64,
    response_time_millis: Vec<u64>,
    error_codes: HashMap<u16, u64>,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            successful_requests: 0,
            failed_requests: 0,
            response_time_millis: Vec::new(),
            error_codes: HashMap::new(),
        }
    }

    pub fn inc_successful_requests(&mut self) {
        self.successful_requests += 1;
    }

    pub fn inc_failed_requests(&mut self) {
        self.failed_requests += 1;
    }

    pub fn add_response_time(&mut self, time: u64) {
        self.response_time_millis.push(time);
    }

    pub fn inc_error_code(&mut self, code: u16) {
        if let Some(count) = self.error_codes.get_mut(&code) {
            *count += 1;
        } else {
            self.error_codes.insert(code, 1);
        }
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let requests_in_general = self.successful_requests + self.failed_requests;

        let mut min_response_time = u64::MAX;
        let mut max_response_time = 0;
        let mut sum = 0;

        for time in self.response_time_millis.iter() {
            if time < &min_response_time {
                min_response_time = *time;
            }

            if time > &max_response_time {
                max_response_time = *time;
            }

            sum += time;
        }

        let average_response_time = sum / self.response_time_millis.len() as u64;

        write!(
            f,
            "\nNumber of requests sent: {}\nSuccessful: {}\nFailed: {}\n",
            requests_in_general, self.successful_requests, self.failed_requests
        )?;

        write!(
            f,
            "\n---\nAverage response time: {} ms\nMax response time: {}\nMin response time: {}\n",
            average_response_time, max_response_time, min_response_time
        )?;

        write!(f, "---\nError codes:\ncode - number")?;
        for (code, number) in self.error_codes.iter() {
            write!(f, "\n{} - {}", code, number)?;
        }

        Ok(())
    }
}

pub struct Worker {
    id: u32,
    commands_channel: Receiver<Commands>,
    api_endpoint: String,
    active: bool,
    keys_fetcher: FileKeysFetcher,
    api: IntegrityVerificationApi,
    stat: Arc<Mutex<Stats>>,
}

impl Worker {
    pub fn new(
        id: u32,
        commands_channel: Receiver<Commands>,
        api_endpoint: String,
        keys_fetcher: FileKeysFetcher,
        stat: Arc<Mutex<Stats>>,
    ) -> Self {
        Self {
            id,
            commands_channel,
            api_endpoint,
            active: false,
            keys_fetcher,
            api: IntegrityVerificationApi::new(),
            stat,
        }
    }

    pub async fn run(&mut self) {
        let mut counter = 5;

        loop {
            if let Ok(has_changed) = self.commands_channel.has_changed() {
                if has_changed {
                    let msg = self.commands_channel.borrow_and_update();

                    match &(*msg) {
                        Commands::Init => {
                            info!("Worker #{} is initialised...", self.id);
                        }
                        Commands::Start(ids) => {
                            for id in ids.iter() {
                                if id == &self.id {
                                    info!("Worker #{} is starting it's job", self.id);
                                    self.active = true;
                                    break;
                                }
                            }
                        }
                        Commands::Stop(ids) => {
                            for id in ids.iter() {
                                if id == &self.id {
                                    return;
                                }
                            }
                        }
                    }
                }
            } else {
                if counter == 0 {
                    info!("Cannot read data from channel");
                    return;
                }
                counter -= 1;

                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }

            if self.active {
                debug!("Worker #{} is sending API request", self.id);
                let (command, arg_key) = self.keys_fetcher.get_random_command();

                let body = {
                    if command == GET_ASSET_METHOD {
                        Body::new(GET_ASSET_METHOD, json!(generate_get_asset_params(arg_key)))
                    } else if command == GET_ASSET_PROOF_METHOD {
                        Body::new(
                            GET_ASSET_PROOF_METHOD,
                            json!(generate_get_asset_proof_params(arg_key)),
                        )
                    } else if command == GET_ASSET_BY_OWNER_METHOD {
                        Body::new(
                            GET_ASSET_BY_OWNER_METHOD,
                            json!(generate_get_assets_by_owner_params(arg_key, None, None)),
                        )
                    } else if command == GET_ASSET_BY_AUTHORITY_METHOD {
                        Body::new(
                            GET_ASSET_BY_AUTHORITY_METHOD,
                            json!(generate_get_assets_by_authority_params(arg_key, None, None)),
                        )
                    } else if command == GET_ASSET_BY_GROUP_METHOD {
                        Body::new(
                            GET_ASSET_BY_GROUP_METHOD,
                            json!(generate_get_assets_by_group_params(arg_key, None, None)),
                        )
                    } else if command == GET_ASSET_BY_CREATOR_METHOD {
                        Body::new(
                            GET_ASSET_BY_CREATOR_METHOD,
                            json!(generate_get_assets_by_creator_params(arg_key, None, None)),
                        )
                    } else if command == GET_TOKEN_ACCOUNTS_BY_OWNER {
                        Body::new(
                            GET_TOKEN_ACCOUNTS,
                            json!(generate_get_token_accounts(Some(arg_key), None)),
                        )
                    } else if command == GET_TOKEN_ACCOUNTS_BY_MINT {
                        Body::new(
                            GET_TOKEN_ACCOUNTS,
                            json!(generate_get_token_accounts(None, Some(arg_key))),
                        )
                    } else if command == GET_TOKEN_ACCOUNTS_BY_OWNER_AND_MINT {
                        let owner_mint: Vec<String> = arg_key
                            .trim_matches(|c| c == '(' || c == ')')
                            .split(';')
                            .map(String::from)
                            .collect();

                        Body::new(
                            GET_TOKEN_ACCOUNTS,
                            json!(generate_get_token_accounts(
                                Some(owner_mint[0].clone()),
                                Some(owner_mint[1].clone())
                            )),
                        )
                    } else if command == GET_SIGNATURES_FOR_ASSET {
                        Body::new(
                            GET_SIGNATURES_FOR_ASSET,
                            json!(generate_get_signatures_for_asset(arg_key)),
                        )
                    } else {
                        panic!("Unknown command was passed")
                    }
                };

                let start = tokio::time::Instant::now();
                let api_call_result = self
                    .api
                    .make_request(&self.api_endpoint, &json!(body).to_string())
                    .await;

                let mut stat = self.stat.lock().await;
                stat.add_response_time(start.elapsed().as_millis() as u64);

                if let Err(e) = api_call_result {
                    if let IntegrityVerificationError::ResponseStatusCode(code) = e {
                        stat.inc_failed_requests();
                        stat.inc_error_code(code);
                    } else {
                        stat.inc_failed_requests();
                    }
                } else {
                    stat.inc_successful_requests();
                }
            }
        }
    }
}

pub async fn run_performance_tests(
    num_of_threads: usize,
    test_duration: u64,
    keys_fetcher: FileKeysFetcher,
    api_url: String,
) {
    let (tx, rx) = watch::channel(Commands::Init);

    let stat = Arc::new(Mutex::new(Stats::new()));

    let mut set = JoinSet::new();
    for id in 0..num_of_threads {
        let keys_fetcher = keys_fetcher.clone();
        let rx = rx.clone();
        let stat = stat.clone();
        let api_url = api_url.clone();
        set.spawn(async move {
            let mut worker = Worker::new(id as u32, rx, api_url, keys_fetcher, stat);

            worker.run().await;

            Ok(())
        });
    }

    let ids: Vec<usize> = (0..num_of_threads).collect();
    let ids: Vec<u32> = ids.iter().map(|x| *x as u32).collect();
    tx.send(Commands::Start(ids.clone())).unwrap();

    tokio::time::sleep(tokio::time::Duration::from_secs(test_duration)).await;

    tx.send(Commands::Stop(ids)).unwrap();

    graceful_stop(&mut set).await;

    println!("{}", stat.lock().await);
}
