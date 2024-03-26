//use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};


use ethers::{core::rand::Rng, prelude::*, utils::keccak256};
use structopt::StructOpt;
use tokio::time::{interval, Duration};
use tokio::{
    sync::mpsc::{self},
    task::JoinHandle,
};

abigen!(
    IPOW,
    r#"[
        function mine(uint256 nonce, address referal) external
        function challenge() external view returns (uint256)
        function difficulty() external view returns (uint256)
        function miningTimes(address account) external view returns (uint256)
        function balanceOf(address account) external view returns (uint256)
    ]"#,
);

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(long)]
    private_key: String,

    #[structopt(long, default_value = "0xe5F8dBf17c9eC8eb327D191dBA74e36970877587")]
    contract_address: String,

    #[structopt(long, default_value = "0xd5A65A20c8071b8fF0Bce2b8F3a4686312b0cB49")]
    referal_address: String,

    #[structopt(long, default_value = "5")]
    worker_count: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();


    println!("Robot AI Miner");

    let provider = Provider::<Http>::try_from("https://babel-api.mainnet.iotex.io")?;
    let wallet = opt.private_key.parse::<LocalWallet>()?.with_chain_id(4689u64);
    let provider = Arc::new(SignerMiddleware::new(provider, wallet));
    println!("🏅 Success init wallet");

    let contract_address: Address = opt.contract_address.parse()?;
    let contract = Arc::new(IPOW::new(contract_address, provider.clone()));

    let referal: Address = opt.referal_address.parse()?;

    let challenge: U256 = contract.challenge().call().await?;
    let difficulty: U256 = contract.difficulty().call().await?;

    let (result_tx, mut result_rx) = mpsc::channel::<U256>(opt.worker_count); // Adjust buffer size as needed
    let hash_counter = Arc::new(AtomicUsize::new(0));

    println!("🏆 Challenge: {}", challenge);
    println!("⛰️  Difficulty: {}", difficulty);

    let difficulty = U256::from(1) << (U256::from(256) - difficulty);
    println!("🎯 Target: {}", difficulty);


    let counter_for_timer = hash_counter.clone();
    let mut interval = interval(Duration::from_secs(1));

    let mut worker_handles: Vec<JoinHandle<()>> = Vec::new();

    for _ in 0..opt.worker_count {
        let counter = hash_counter.clone();
        let sender = result_tx.clone();
        let addr = provider.signer().address();

        let handle = tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                return mine_worker(addr, challenge, difficulty, counter);
            })
            .await
            .unwrap();

            sender.send(result).await.unwrap();
        });

        worker_handles.push(handle);
    }

    let speed_bar = ProgressBar::new(100);
    speed_bar.set_style(
        ProgressStyle::default_bar()
            .template("{prefix:.bold} {spinner:.green} {msg}")
            .unwrap()
            .progress_chars("##-"),
    );
    speed_bar.set_prefix("🚄 Speed");


    loop {

        tokio::select! {
            _ = interval.tick()=>{
                let total_hash_count = counter_for_timer.swap(0, Ordering::SeqCst);
                let hashes_per_second = total_hash_count as f64 / 1000.0;
                speed_bar.set_message(format!("Hash per second: {:.2} K/s", hashes_per_second));
            },
            nonce = result_rx.recv() => {
                if let Some(nonce) = nonce {
                    println!("✅ Find the nonce: {}", nonce);
                    let contract = contract.clone();
                    let addr = provider.signer().address();

                    tokio::spawn(async move{

                        let gas_price = ethers::core::types::U256::from(1_010_000_000_000u64);
                        let call = contract.mine(nonce, referal).gas_price(gas_price);
                        let result = call.send().await;
                        match result {
                            Ok(tx) => match tx.await {
                                Ok(Some(receipt)) => {
                                    println!("Transaction successful with hash: {:?}", receipt.transaction_hash);
                                },
                                Ok(None) => eprintln!("Transaction might have been dropped or replaced"),
                                Err(e) => eprintln!("Transaction execution error: {:?}", e),
                            },
                            Err(e) => eprintln!("Transaction send error: {:?}", e),
                        }

                        let mint_times: U256 = match contract.mining_times(addr).call().await {
                            Ok(mint_times) => mint_times,
                            Err(e) => {
                                eprintln!("Error calling mining_times: {:?}", e);
                                U256::from(0)
                                //std::process::exit(1); // Exit with error code
                            },
                        };
                        println!("mint_times: {}", mint_times);
                        if mint_times >= U256::from(25) {
                            println!("already minted 100 times, exit process");
                            std::process::exit(0);
                        }

                    });

                }
            }
        }

    }
}

fn mine_worker(
    from: Address,
    challenge: U256,
    target: U256,
    hash_counter: Arc<AtomicUsize>,
) -> U256 {
    loop {
        let mut data = Vec::new();
        let challenge_bytes = {
            let mut buf = [0u8; 32];
            challenge.to_big_endian(&mut buf);
            buf
        };
        data.extend_from_slice(&challenge_bytes);
        data.extend_from_slice(from.as_bytes());

        let nonce = rand::thread_rng().gen::<[u8; 32]>();
        let nonce_big_int = U256::from_big_endian(&nonce);

        let nonce_bytes = {
            let mut buf = [0u8; 32];
            nonce_big_int.to_big_endian(&mut buf);
            buf
        };
        data.extend_from_slice(&nonce_bytes);
        // Hash the data
        let hash = keccak256(&data);
        let hash_val = U256::from_big_endian(&hash);
        // Check if hash is less than target
        if hash_val < target {
            return nonce_big_int;
        }

        hash_counter.fetch_add(1, Ordering::SeqCst);
    }
}
