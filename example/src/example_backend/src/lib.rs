use std::str::FromStr;

use borsh::{to_vec, BorshSerialize};
use solana_client_icp::{
    solana_sdk::{
        instruction::{AccountMeta, Instruction},
        message::Message,
        pubkey::Pubkey,
        signer::{
            threshold_signer::{SchnorrKeyIds, ThresholdSigner},
            Signer,
        },
        transaction::Transaction,
    },
    CallOptions, WasmClient,
};

#[ic_cdk::update]
async fn balance() -> u64 {
    let pubkey = Pubkey::from_str("updtkJ8HAhh3rSkBCd3p9Z1Q74yJW4rMhSbScRskDPM").unwrap();
    let balance = client()
        .get_balance(&pubkey, CallOptions::default())
        .await
        .unwrap();
    ic_cdk::println!("Balance: {:?}", balance);
    balance
}
async fn signer() -> ThresholdSigner {
    let signer = ThresholdSigner::new(SchnorrKeyIds::TestKeyLocalDevelopment)
        .await
        .unwrap();
    signer
}
fn discriminator(name: &str) -> [u8; 8] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(format!("global:{}", name));
    let hash = hasher.finalize();
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}
#[ic_cdk::update]
async fn update_state() {
    let entry = Entry {
        key: "counter".to_string(),
        value: 12345678u128.to_le_bytes().to_vec(),
    };
    let args = UpdateStateArgs {
        entries: vec![entry],
    };
    let mut data = Vec::new();
    data.extend_from_slice(&discriminator("update_state"));
    data.extend_from_slice(&to_vec(&args).unwrap());
    let program_id = Pubkey::from_str("B71fttHMcvSd5U3erbsJMqc2BLD9uWdWWAEyPz5BhaLq").unwrap();
    let state_pubkey = Pubkey::from_str("TWkKWRhCkq7MvAGK1YqumRrXVwtisZ6qLkyMnJXP4qk").unwrap();
    let payer = signer().await;
    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(state_pubkey, false),
            AccountMeta::new(payer.pubkey(), true),
        ],
        data,
    };

    let recent_blockhash = client()
        .get_latest_blockhash(CallOptions::default())
        .await
        .unwrap();
    let message = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], message, recent_blockhash).await;

    // 送信
    let sig = client().send_transaction(&tx, CallOptions::default()).await;
    ic_cdk::println!("Signature: {:?}", sig);
}

#[ic_cdk::update]
async fn pubkey() -> String {
    let pubkey = signer().await.pubkey();
    ic_cdk::println!("Pubkey: {:?}", pubkey.to_string());
    pubkey.to_string()
}
fn client() -> WasmClient {
    WasmClient::new("https://api.devnet.solana.com")
}
#[derive(BorshSerialize)]
struct Entry {
    key: String,
    value: Vec<u8>,
}

#[derive(BorshSerialize)]
struct UpdateStateArgs {
    entries: Vec<Entry>,
}
