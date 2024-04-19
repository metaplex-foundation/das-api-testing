use crate::config::setup_config;
use crate::diff_checker::{
    DiffChecker, GET_ASSET_BY_AUTHORITY_METHOD, GET_ASSET_BY_CREATOR_METHOD,
    GET_ASSET_BY_GROUP_METHOD, GET_ASSET_BY_OWNER_METHOD, GET_ASSET_METHOD, GET_ASSET_PROOF_METHOD,
    GET_SIGNATURES_FOR_ASSET, GET_TOKEN_ACCOUNTS_BY_MINT, GET_TOKEN_ACCOUNTS_BY_OWNER,
    GET_TOKEN_ACCOUNTS_BY_OWNER_AND_MINT,
};
use crate::error::IntegrityVerificationError;
use crate::file_keys_fetcher::FileKeysFetcher;
use crate::graceful_stop::{graceful_stop, listen_shutdown};
use crate::interfaces::IntegrityVerificationKeysFetcher;
use clap::Parser;
use performance_measurement::run_performance_tests;
use std::sync::Arc;
use tokio::task::{JoinError, JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

mod api;
mod api_req_params;
mod config;
mod diff_checker;
mod error;
mod file_keys_fetcher;
mod graceful_stop;
mod interfaces;
mod merkle_tree;
mod params_generation;
mod performance_measurement;
mod requests;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value_t = String::new())]
    config_path: String,
    #[arg(short, long)]
    test_type: TestsType,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum TestsType {
    Integrity,
    Performance,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), IntegrityVerificationError> {
    let args = Args::parse();
    env_logger::init();
    info!("DAS-API tests start");

    let config = setup_config(args.config_path.as_str())?;

    let keys_fetcher = FileKeysFetcher::new(&config.testing_file_path.clone())
        .await
        .unwrap();

    match args.test_type {
        TestsType::Integrity => {
            let mut tasks = JoinSet::new();
            let cancel_token = CancellationToken::new();

            let diff_checker = Arc::new(
                DiffChecker::new(
                    &config,
                    FileKeysFetcher::new(&config.testing_file_path.clone())
                        .await
                        .unwrap(),
                )
                .await?,
            );

            listen_shutdown(cancel_token.clone()).await;
            run_tests(&mut tasks, diff_checker.clone(), cancel_token.clone()).await;
            diff_checker.show_results().await;
        }
        TestsType::Performance => {
            run_performance_tests(
                config.num_of_virtual_users,
                config.test_duration_time,
                keys_fetcher,
                config.testing_host,
            )
            .await;
        }
    }

    Ok(())
}

macro_rules! spawn_test {
    ($tasks:ident, $diff_checker:ident, $method:ident, $test_label:expr, $cancel_token:expr) => {{
        info!("{} tests start", &$test_label);
        let diff_checker_clone = $diff_checker.clone();
        let cancel_token_clone = $cancel_token.clone();
        $tasks.spawn(tokio::spawn(async move {
            tokio::select! {
                _ = async {
                    if let Err(e) = diff_checker_clone.$method().await {
                        error!("Fetch keys: {}", e);
                    }
                } => {},
                _ = cancel_token_clone.cancelled() => {}
            };
        }));
    }};
}

async fn run_tests<T>(
    tasks: &mut JoinSet<Result<(), JoinError>>,
    diff_checker: Arc<DiffChecker<T>>,
    cancel_token: CancellationToken,
) where
    T: IntegrityVerificationKeysFetcher + Send + Sync + 'static,
{
    spawn_test!(
        tasks,
        diff_checker,
        check_get_asset,
        GET_ASSET_METHOD,
        cancel_token
    );
    spawn_test!(
        tasks,
        diff_checker,
        check_get_asset_proof,
        GET_ASSET_PROOF_METHOD,
        cancel_token
    );
    spawn_test!(
        tasks,
        diff_checker,
        check_get_asset_by_owner,
        GET_ASSET_BY_OWNER_METHOD,
        cancel_token
    );
    spawn_test!(
        tasks,
        diff_checker,
        check_get_asset_by_authority,
        GET_ASSET_BY_AUTHORITY_METHOD,
        cancel_token
    );
    spawn_test!(
        tasks,
        diff_checker,
        check_get_asset_by_creator,
        GET_ASSET_BY_CREATOR_METHOD,
        cancel_token
    );
    spawn_test!(
        tasks,
        diff_checker,
        check_get_asset_by_group,
        GET_ASSET_BY_GROUP_METHOD,
        cancel_token
    );
    spawn_test!(
        tasks,
        diff_checker,
        check_get_token_accounts_by_owner,
        GET_TOKEN_ACCOUNTS_BY_OWNER,
        cancel_token
    );
    spawn_test!(
        tasks,
        diff_checker,
        check_get_token_accounts_by_mint,
        GET_TOKEN_ACCOUNTS_BY_MINT,
        cancel_token
    );
    spawn_test!(
        tasks,
        diff_checker,
        check_get_token_accounts_by_owner_and_mint,
        GET_TOKEN_ACCOUNTS_BY_OWNER_AND_MINT,
        cancel_token
    );
    spawn_test!(
        tasks,
        diff_checker,
        check_get_signatures_for_asset,
        GET_SIGNATURES_FOR_ASSET,
        cancel_token
    );
    graceful_stop(tasks).await;
}
