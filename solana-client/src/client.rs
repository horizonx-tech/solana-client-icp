use serde::de::DeserializeOwned;
use solana_extra_wasm::{
    account_decoder::{
        parse_token::UiTokenAccount,
        parse_token::{TokenAccountType, UiTokenAmount},
        UiAccountData, UiAccountEncoding,
    },
    transaction_status::{
        EncodedConfirmedTransactionWithStatusMeta, TransactionConfirmationStatus, UiConfirmedBlock,
        UiTransactionEncoding,
    },
};
use solana_sdk::{
    account::Account,
    clock::{Epoch, Slot, UnixTimestamp},
    commitment_config::{CommitmentConfig, CommitmentLevel},
    epoch_info::EpochInfo,
    epoch_schedule::EpochSchedule,
    hash::Hash,
    message::Message,
    pubkey::Pubkey,
    signature::Signature,
    transaction::Transaction,
};

use crate::{
    constants::MAX_RETRIES,
    methods::*,
    provider::{CallOptions, Provider},
    utils::{
        rpc_config::{
            GetConfirmedSignaturesForAddress2Config, RpcAccountInfoConfig, RpcBlockConfig,
            RpcBlockProductionConfig, RpcContextConfig, RpcEpochConfig, RpcGetVoteAccountsConfig,
            RpcKeyedAccount, RpcLargestAccountsConfig, RpcLeaderScheduleConfig,
            RpcProgramAccountsConfig, RpcSendTransactionConfig, RpcSignaturesForAddressConfig,
            RpcSimulateTransactionConfig, RpcSupplyConfig, RpcTransactionConfig,
        },
        rpc_filter::RpcTokenAccountsFilter,
        rpc_response::{
            RpcAccountBalance, RpcBlockProduction, RpcConfirmedTransactionStatusWithSignature,
            RpcInflationGovernor, RpcInflationRate, RpcInflationReward, RpcLeaderSchedule,
            RpcPerfSample, RpcSupply, RpcVersionInfo, RpcVoteAccountStatus,
        },
    },
    ClientError, ClientResponse, ClientResult,
};

pub struct WasmClient {
    provider: Provider,
    commitment_config: CommitmentConfig,
    #[cfg(feature = "pubsub")]
    pub(crate) ws: crate::pubsub::WasmWebSocket,
}

impl WasmClient {
    /// Create a [`WasmClient`].
    ///
    /// Default commitment is `confirmed` unlike default Solana Client.
    pub fn new(endpoint: &str) -> Self {
        Self {
            provider: Provider::new(endpoint),
            commitment_config: CommitmentConfig::confirmed(),
            #[cfg(feature = "pubsub")]
            ws: crate::pubsub::WasmWebSocket::new(endpoint),
        }
    }

    pub fn new_with_commitment(endpoint: &str, commitment_config: CommitmentConfig) -> Self {
        Self {
            provider: Provider::new(endpoint),
            commitment_config,
            #[cfg(feature = "pubsub")]
            ws: crate::pubsub::WasmWebSocket::new(endpoint),
        }
    }

    pub fn commitment(&self) -> CommitmentLevel {
        self.commitment_config.commitment
    }

    pub fn commitment_config(&self) -> CommitmentConfig {
        self.commitment_config
    }

    async fn send<T: Method, R: DeserializeOwned>(
        &self,
        request: T,
        opts: CallOptions,
    ) -> ClientResult<R> {
        let Provider::Http(provider) = &self.provider;
        let r = provider.send(&request, opts).await?;
        Ok(r.result)
    }

    pub async fn get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<u64> {
        let request = GetBalanceRequest::new_with_config(*pubkey, commitment_config);
        let response: GetBalanceResponse = self.send(request, opts).await?;
        Ok(response.value)
    }

    pub async fn get_balance(&self, pubkey: &Pubkey, opts: CallOptions) -> ClientResult<u64> {
        self.get_balance_with_commitment(pubkey, self.commitment_config(), opts)
            .await
    }

    pub async fn request_airdrop(
        &self,
        pubkey: &Pubkey,
        lamports: u64,
        opts: CallOptions,
    ) -> ClientResult<Signature> {
        let request = RequestAirdropRequest::new(*pubkey, lamports);
        let response: ClientResponse<RequestAirdropResponse> = self.send(request, opts).await?;

        Ok(response.result.into())
    }

    pub async fn get_signature_statuses(
        &self,
        signatures: &[Signature],
        opts: CallOptions,
    ) -> ClientResult<Vec<Option<SignatureStatusesValue>>> {
        let request = GetSignatureStatusesRequest::new(signatures.into());
        let response: GetSignatureStatusesResponse = self.send(request, opts).await?;

        Ok(response.value)
    }

    pub async fn get_transaction_with_config(
        &self,
        signature: &Signature,
        config: RpcTransactionConfig,
        opts: CallOptions,
    ) -> ClientResult<EncodedConfirmedTransactionWithStatusMeta> {
        let request = GetTransactionRequest::new_with_config(*signature, config);
        let response: GetTransactionResponse = self.send(request, opts).await?;

        match response.into() {
            Some(result) => Ok(result),
            None => Err(ClientError::new(format!(
                "Signature {signature} not found."
            ))),
        }
    }

    pub async fn get_account_with_config(
        &self,
        pubkey: &Pubkey,
        config: RpcAccountInfoConfig,
        opts: CallOptions,
    ) -> ClientResult<Option<Account>> {
        let request = GetAccountInfoRequest::new_with_config(*pubkey, config);
        let response: GetAccountInfoResponse = self.send(request, opts).await?;

        match response.value {
            Some(ui_account) => Ok(ui_account.decode()),
            None => Ok(None),
        }
    }

    pub async fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<Option<Account>> {
        self.get_account_with_config(
            pubkey,
            RpcAccountInfoConfig {
                commitment: Some(commitment_config),
                encoding: Some(UiAccountEncoding::Base64),
                ..Default::default()
            },
            opts,
        )
        .await
    }

    pub async fn get_account(&self, pubkey: &Pubkey, opts: CallOptions) -> ClientResult<Account> {
        self.get_account_with_commitment(pubkey, self.commitment_config(), opts)
            .await?
            .ok_or_else(|| ClientError::new(format!("Account {} not found.", pubkey)))
    }

    pub async fn get_account_data(
        &self,
        pubkey: &Pubkey,
        opts: CallOptions,
    ) -> ClientResult<Vec<u8>> {
        Ok(self.get_account(pubkey, opts).await?.data)
    }

    pub async fn get_latest_blockhash_with_config(
        &self,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<(Hash, u64)> {
        let request = GetLatestBlockhashRequest::new_with_config(commitment_config);
        let response: GetLatestBlockhashResponse = self.send(request, opts).await?;

        let hash = response
            .value
            .blockhash
            .parse()
            .map_err(|_| ClientError::new("Hash not parsable."))?;

        Ok((hash, response.value.last_valid_block_height))
    }

    pub async fn get_latest_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<(Hash, u64)> {
        self.get_latest_blockhash_with_config(commitment_config, opts)
            .await
    }

    pub async fn get_latest_blockhash(&self, opts: CallOptions) -> ClientResult<Hash> {
        let result = self
            .get_latest_blockhash_with_commitment(self.commitment_config(), opts)
            .await?;

        Ok(result.0)
    }

    pub async fn is_blockhash_valid(
        &self,
        blockhash: &Hash,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<bool> {
        let request = IsBlockhashValidRequest::new_with_config(
            *blockhash,
            RpcContextConfig {
                commitment: Some(commitment_config),
                min_context_slot: None,
            },
        );
        let response: IsBlockhashValidResponse = self.send(request, opts).await?;

        Ok(response.value)
    }

    pub async fn get_minimum_balance_for_rent_exemption(
        &self,
        data_len: usize,
        opts: CallOptions,
    ) -> ClientResult<u64> {
        let request = GetMinimumBalanceForRentExemptionRequest::new(data_len);
        let response: GetMinimumBalanceForRentExemptionResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_fee_for_message(
        &self,
        message: &Message,
        opts: CallOptions,
    ) -> ClientResult<u64> {
        let request = GetFeeForMessageRequest::new(message.to_owned());
        let response: GetFeeForMessageResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn send_transaction_with_config(
        &self,
        transaction: &Transaction,
        config: RpcSendTransactionConfig,
        opts: CallOptions,
    ) -> ClientResult<Signature> {
        let request = SendTransactionRequest::new_with_config(transaction.to_owned(), config);
        let response: SendTransactionResponse = self.send(request, opts).await?;
        let signature: Signature = response.into();

        // A mismatching RPC response signature indicates an issue with the RPC node, and
        // should not be passed along to confirmation methods. The transaction may or may
        // not have been submitted to the cluster, so callers should verify the success of
        // the correct transaction signature independently.
        if signature != transaction.signatures[0] {
            Err(ClientError::new(format!(
                "RPC node returned mismatched signature {:?}, expected {:?}",
                signature, transaction.signatures[0]
            )))
        } else {
            Ok(transaction.signatures[0])
        }
    }

    pub async fn send_transaction(
        &self,
        transaction: &Transaction,
        opts: CallOptions,
    ) -> ClientResult<Signature> {
        self.send_transaction_with_config(
            transaction,
            RpcSendTransactionConfig {
                preflight_commitment: Some(self.commitment()),
                encoding: Some(UiTransactionEncoding::Base64),
                ..Default::default()
            },
            opts,
        )
        .await
    }

    pub async fn confirm_transaction_with_commitment(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<bool> {
        let mut is_success = false;
        for _ in 0..MAX_RETRIES {
            let signature_statuses = self
                .get_signature_statuses(&[*signature], opts.clone())
                .await?;

            if let Some(signature_status) = signature_statuses[0].as_ref() {
                if signature_status.confirmation_status.is_some() {
                    let current_commitment = signature_status.confirmation_status.as_ref().unwrap();

                    let commitment_matches = match commitment_config.commitment {
                        CommitmentLevel::Finalized => {
                            matches!(current_commitment, TransactionConfirmationStatus::Finalized)
                        }
                        CommitmentLevel::Confirmed => matches!(
                            current_commitment,
                            TransactionConfirmationStatus::Finalized
                                | TransactionConfirmationStatus::Confirmed
                        ),
                        _ => true,
                    };
                    if commitment_matches {
                        is_success = signature_status.err.is_none();
                        break;
                    }
                }
            }

            //            sleep(SLEEP_MS).await;
        }

        Ok(is_success)
    }

    pub async fn confirm_transaction(
        &self,
        signature: &Signature,
        opts: CallOptions,
    ) -> ClientResult<bool> {
        self.confirm_transaction_with_commitment(signature, self.commitment_config(), opts)
            .await
    }

    pub async fn send_and_confirm_transaction_with_config(
        &self,
        transaction: &Transaction,
        commitment_config: CommitmentConfig,
        config: RpcSendTransactionConfig,
        opts: CallOptions,
    ) -> ClientResult<Signature> {
        let tx_hash = self
            .send_transaction_with_config(transaction, config, opts.clone())
            .await?;

        self.confirm_transaction_with_commitment(&tx_hash, commitment_config, opts)
            .await?;

        Ok(tx_hash)
    }

    pub async fn send_and_confirm_transaction_with_commitment(
        &self,
        transaction: &Transaction,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<Signature> {
        self.send_and_confirm_transaction_with_config(
            transaction,
            commitment_config,
            RpcSendTransactionConfig {
                preflight_commitment: Some(commitment_config.commitment),
                encoding: Some(UiTransactionEncoding::Base64),
                ..Default::default()
            },
            opts,
        )
        .await
    }

    pub async fn send_and_confirm_transaction(
        &self,
        transaction: &Transaction,
        opts: CallOptions,
    ) -> ClientResult<Signature> {
        self.send_and_confirm_transaction_with_commitment(
            transaction,
            self.commitment_config(),
            opts,
        )
        .await
    }

    pub async fn get_program_accounts_with_config(
        &self,
        pubkey: &Pubkey,
        config: RpcProgramAccountsConfig,
        opts: CallOptions,
    ) -> ClientResult<Vec<(Pubkey, Account)>> {
        let commitment = config
            .account_config
            .commitment
            .unwrap_or_else(|| self.commitment_config());
        let account_config = RpcAccountInfoConfig {
            commitment: Some(commitment),
            ..config.account_config
        };
        let config = RpcProgramAccountsConfig {
            account_config,
            ..config
        };

        let request = GetProgramAccountsRequest::new_with_config(*pubkey, config);
        let response: GetProgramAccountsResponse = self.send(request, opts).await?;

        // Parse keyed accounts
        let accounts = response
            .keyed_accounts()
            .ok_or_else(|| ClientError::new("Program account doesn't exist."))?;

        let mut pubkey_accounts: Vec<(Pubkey, Account)> = Vec::with_capacity(accounts.len());
        for RpcKeyedAccount { pubkey, account } in accounts.iter() {
            let pubkey = pubkey
                .parse()
                .map_err(|_| ClientError::new(format!("{pubkey} is not a valid pubkey.")))?;
            pubkey_accounts.push((
                pubkey,
                account
                    .decode()
                    .ok_or_else(|| ClientError::new(format!("Unable to decode {pubkey}")))?,
            ));
        }
        Ok(pubkey_accounts)
    }

    pub async fn get_program_accounts(
        &self,
        pubkey: &Pubkey,
        opts: CallOptions,
    ) -> ClientResult<Vec<(Pubkey, Account)>> {
        self.get_program_accounts_with_config(
            pubkey,
            RpcProgramAccountsConfig {
                account_config: RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::Base64),
                    ..RpcAccountInfoConfig::default()
                },
                ..RpcProgramAccountsConfig::default()
            },
            opts,
        )
        .await
    }

    pub async fn get_slot_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<Slot> {
        let request = GetSlotRequest::new_with_config(commitment_config);
        let response: GetSlotResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_slot(&self, opts: CallOptions) -> ClientResult<Slot> {
        self.get_slot_with_commitment(self.commitment_config(), opts)
            .await
    }

    pub async fn get_block_with_config(
        &self,
        slot: Slot,
        config: RpcBlockConfig,
        opts: CallOptions,
    ) -> ClientResult<UiConfirmedBlock> {
        let request = GetBlockRequest::new_with_config(slot, config);
        let response: GetBlockResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_version(&self, opts: CallOptions) -> ClientResult<RpcVersionInfo> {
        let request = GetVersionRequest;
        let response: GetVersionResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_first_available_block(&self, opts: CallOptions) -> ClientResult<Slot> {
        let request = GetFirstAvailableBlockRequest;
        let response: GetFirstAvailableBlockResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_block_time(
        &self,
        slot: Slot,
        opts: CallOptions,
    ) -> ClientResult<UnixTimestamp> {
        let request = GetBlockTimeRequest::new(slot);
        let response: GetBlockTimeResponse = self.send(request, opts).await?;

        let maybe_ts: Option<UnixTimestamp> = response.into();
        match maybe_ts {
            Some(ts) => Ok(ts),
            None => Err(ClientError::new(format!("Block Not Found: slot={}", slot))),
        }
    }

    pub async fn get_block_height_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<u64> {
        let request = GetBlockHeightRequest::new_with_config(commitment_config);
        let response: GetBlockHeightResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_block_height(&self, opts: CallOptions) -> ClientResult<u64> {
        self.get_block_height_with_commitment(self.commitment_config(), opts)
            .await
    }

    pub async fn get_genesis_hash(&self, opts: CallOptions) -> ClientResult<Hash> {
        let request = GetGenesisHashRequest;
        let response: GetGenesisHashResponse = self.send(request, opts).await?;

        let hash_string: String = response.into();
        let hash = hash_string
            .parse()
            .map_err(|_| ClientError::new("Hash is not parseable."))?;

        Ok(hash)
    }

    pub async fn get_epoch_info_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<EpochInfo> {
        let request = GetEpochInfoRequest::new_with_config(commitment_config);
        let response: GetEpochInfoResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_epoch_info(&self, opts: CallOptions) -> ClientResult<EpochInfo> {
        self.get_epoch_info_with_commitment(self.commitment_config(), opts)
            .await
    }

    pub async fn get_recent_performance_samples(
        &self,
        limit: Option<usize>,
        opts: CallOptions,
    ) -> ClientResult<Vec<RpcPerfSample>> {
        let request = GetRecentPerformanceSamplesRequest { limit };
        let response: GetRecentPerformanceSamplesResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_blocks_with_limit_and_commitment(
        &self,
        start_slot: Slot,
        limit: usize,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<Vec<Slot>> {
        let request =
            GetBlocksWithLimitRequest::new_with_config(start_slot, limit, commitment_config);
        let response: GetBlocksWithLimitResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_blocks_with_limit(
        &self,
        start_slot: Slot,
        limit: usize,
        opts: CallOptions,
    ) -> ClientResult<Vec<Slot>> {
        self.get_blocks_with_limit_and_commitment(start_slot, limit, self.commitment_config(), opts)
            .await
    }

    pub async fn get_largest_accounts_with_config(
        &self,
        config: RpcLargestAccountsConfig,
        opts: CallOptions,
    ) -> ClientResult<Vec<RpcAccountBalance>> {
        let config = RpcLargestAccountsConfig {
            commitment: config.commitment,
            ..config
        };

        let request = GetLargestAccountsRequest::new_with_config(config);
        let response: GetLargestAccountsResponse = self.send(request, opts).await?;

        Ok(response.value)
    }

    pub async fn get_supply_with_config(
        &self,
        config: RpcSupplyConfig,
        opts: CallOptions,
    ) -> ClientResult<RpcSupply> {
        let request = GetSupplyRequest::new_with_config(config);
        let response: GetSupplyResponse = self.send(request, opts).await?;

        Ok(response.value)
    }

    pub async fn get_supply_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<RpcSupply> {
        self.get_supply_with_config(
            RpcSupplyConfig {
                commitment: Some(commitment_config),
                exclude_non_circulating_accounts_list: false,
            },
            opts,
        )
        .await
    }

    pub async fn get_supply(&self, opts: CallOptions) -> ClientResult<RpcSupply> {
        self.get_supply_with_commitment(self.commitment_config(), opts)
            .await
    }

    pub async fn get_transaction_count_with_config(
        &self,
        config: RpcContextConfig,
        opts: CallOptions,
    ) -> ClientResult<u64> {
        let request = GetTransactionCountRequest::new_with_config(config);
        let response: GetTransactionCountResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_transaction_count_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<u64> {
        self.get_transaction_count_with_config(
            RpcContextConfig {
                commitment: Some(commitment_config),
                min_context_slot: None,
            },
            opts,
        )
        .await
    }

    pub async fn get_transaction_count(&self, opts: CallOptions) -> ClientResult<u64> {
        self.get_transaction_count_with_commitment(self.commitment_config(), opts)
            .await
    }

    pub async fn get_multiple_accounts_with_config(
        &self,
        pubkeys: &[Pubkey],
        config: RpcAccountInfoConfig,
        opts: CallOptions,
    ) -> ClientResult<Vec<Option<Account>>> {
        let config = RpcAccountInfoConfig {
            commitment: config.commitment,
            ..config
        };

        let request = GetMultipleAccountsRequest::new_with_config(pubkeys.to_vec(), config);
        let response: GetMultipleAccountsResponse = self.send(request, opts).await?;

        Ok(response
            .value
            .iter()
            .filter(|maybe_acc| maybe_acc.is_some())
            .map(|acc| acc.clone().unwrap().decode())
            .collect())
    }

    pub async fn get_multiple_accounts_with_commitment(
        &self,
        pubkeys: &[Pubkey],
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<Vec<Option<Account>>> {
        self.get_multiple_accounts_with_config(
            pubkeys,
            RpcAccountInfoConfig {
                commitment: Some(commitment_config),
                ..RpcAccountInfoConfig::default()
            },
            opts,
        )
        .await
    }

    pub async fn get_multiple_accounts(
        &self,
        pubkeys: &[Pubkey],
        opts: CallOptions,
    ) -> ClientResult<Vec<Option<Account>>> {
        self.get_multiple_accounts_with_commitment(pubkeys, self.commitment_config(), opts)
            .await
    }

    pub async fn get_cluster_nodes(
        &self,
        opts: CallOptions,
    ) -> ClientResult<Vec<RpcContactInfoWasm>> {
        let request = GetClusterNodesRequest;
        let response: GetClusterNodesResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_vote_accounts_with_config(
        &self,
        config: RpcGetVoteAccountsConfig,
        opts: CallOptions,
    ) -> ClientResult<RpcVoteAccountStatus> {
        let request = GetVoteAccountsRequest::new_with_config(config);
        let response: GetVoteAccountsResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_vote_accounts_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<RpcVoteAccountStatus> {
        self.get_vote_accounts_with_config(
            RpcGetVoteAccountsConfig {
                commitment: Some(commitment_config),
                ..Default::default()
            },
            opts,
        )
        .await
    }

    pub async fn get_vote_accounts(&self, opts: CallOptions) -> ClientResult<RpcVoteAccountStatus> {
        self.get_vote_accounts_with_commitment(self.commitment_config(), opts)
            .await
    }

    pub async fn get_epoch_schedule(&self, opts: CallOptions) -> ClientResult<EpochSchedule> {
        let request = GetEpochScheduleRequest;
        let response: GetEpochScheduleResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_signatures_for_address_with_config(
        &self,
        address: &Pubkey,
        config: GetConfirmedSignaturesForAddress2Config,
        opts: CallOptions,
    ) -> ClientResult<Vec<RpcConfirmedTransactionStatusWithSignature>> {
        let config = RpcSignaturesForAddressConfig {
            before: config.before.map(|signature| signature.to_string()),
            until: config.until.map(|signature| signature.to_string()),
            limit: config.limit,
            commitment: config.commitment,
            min_context_slot: None,
        };

        let request = GetSignaturesForAddressRequest::new_with_config(*address, config);
        let response: GetSignaturesForAddressResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn minimum_ledger_slot(&self, opts: CallOptions) -> ClientResult<Slot> {
        let request = MinimumLedgerSlotRequest;
        let response: MinimumLedgerSlotResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_blocks_with_commitment(
        &self,
        start_slot: Slot,
        end_slot: Option<Slot>,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<Vec<Slot>> {
        let request = GetBlocksRequest::new_with_config(start_slot, end_slot, commitment_config);
        let response: GetBlocksResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_blocks(
        &self,
        start_slot: Slot,
        end_slot: Option<Slot>,
        opts: CallOptions,
    ) -> ClientResult<Vec<Slot>> {
        self.get_blocks_with_commitment(start_slot, end_slot, self.commitment_config(), opts)
            .await
    }

    pub async fn get_leader_schedule_with_config(
        &self,
        slot: Option<Slot>,
        config: RpcLeaderScheduleConfig,
        opts: CallOptions,
    ) -> ClientResult<Option<RpcLeaderSchedule>> {
        let request = match slot {
            Some(s) => GetLeaderScheduleRequest::new_with_slot_and_config(s, config),
            None => GetLeaderScheduleRequest::new_with_config(config),
        };
        let response: GetLeaderScheduleResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_leader_schedule_with_commitment(
        &self,
        slot: Option<Slot>,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<Option<RpcLeaderSchedule>> {
        self.get_leader_schedule_with_config(
            slot,
            RpcLeaderScheduleConfig {
                commitment: Some(commitment_config),
                ..Default::default()
            },
            opts,
        )
        .await
    }

    pub async fn get_block_production_with_config(
        &self,
        config: RpcBlockProductionConfig,
        opts: CallOptions,
    ) -> ClientResult<RpcBlockProduction> {
        let request = GetBlockProductionRequest::new_with_config(config);
        let response: GetBlockProductionResponse = self.send(request, opts).await?;

        Ok(response.value)
    }

    pub async fn get_block_production_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<RpcBlockProduction> {
        self.get_block_production_with_config(
            RpcBlockProductionConfig {
                commitment: Some(commitment_config),
                ..Default::default()
            },
            opts,
        )
        .await
    }

    pub async fn get_block_production(
        &self,
        opts: CallOptions,
    ) -> ClientResult<RpcBlockProduction> {
        self.get_block_production_with_commitment(self.commitment_config(), opts)
            .await
    }

    pub async fn get_inflation_governor_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<RpcInflationGovernor> {
        let request = GetInflationGovernorRequest::new_with_config(commitment_config);
        let response: GetInflationGovernorResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_inflation_governor(
        &self,
        opts: CallOptions,
    ) -> ClientResult<RpcInflationGovernor> {
        self.get_inflation_governor_with_commitment(self.commitment_config(), opts)
            .await
    }

    pub async fn get_inflation_rate(&self, opts: CallOptions) -> ClientResult<RpcInflationRate> {
        let request = GetInflationRateRequest;
        let response: GetInflationRateResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_inflation_reward_with_config(
        &self,
        addresses: &[Pubkey],
        epoch: Option<Epoch>,
        opts: CallOptions,
    ) -> ClientResult<Vec<Option<RpcInflationReward>>> {
        let request = GetInflationRewardRequest::new_with_config(
            addresses.to_vec(),
            RpcEpochConfig {
                commitment: Some(self.commitment_config()),
                epoch,
                ..Default::default()
            },
        );
        let response: GetInflationRewardResponse = self.send(request, opts).await?;

        Ok(response.into())
    }

    pub async fn get_inflation_reward(
        &self,
        addresses: &[Pubkey],
        opts: CallOptions,
    ) -> ClientResult<Vec<Option<RpcInflationReward>>> {
        self.get_inflation_reward_with_config(addresses, None, opts)
            .await
    }

    pub async fn get_token_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<Option<UiTokenAccount>> {
        let config = RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::JsonParsed),
            commitment: Some(commitment_config),
            data_slice: None,
            min_context_slot: None,
        };

        let request = GetAccountInfoRequest::new_with_config(*pubkey, config);
        let response: GetAccountInfoResponse = self.send(request, opts).await?;

        if let Some(acc) = response.value {
            if let UiAccountData::Json(account_data) = acc.data {
                let token_account_type: TokenAccountType =
                    match serde_json::from_value(account_data.parsed) {
                        Ok(t) => t,
                        Err(e) => return Err(ClientError::new(e.to_string())),
                    };

                if let TokenAccountType::Account(token_account) = token_account_type {
                    return Ok(Some(token_account));
                }
            }
        }

        Err(ClientError::new(format!(
            "AccountNotFound: pubkey={}",
            pubkey
        )))
    }

    pub async fn get_token_account(
        &self,
        pubkey: &Pubkey,
        opts: CallOptions,
    ) -> ClientResult<Option<UiTokenAccount>> {
        self.get_token_account_with_commitment(pubkey, self.commitment_config(), opts)
            .await
    }

    pub async fn get_token_accounts_by_owner_with_commitment(
        &self,
        owner: &Pubkey,
        token_account_filter: RpcTokenAccountsFilter,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<Vec<RpcKeyedAccount>> {
        let config = RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::JsonParsed),
            commitment: Some(commitment_config),
            data_slice: None,
            min_context_slot: None,
        };

        let request =
            GetTokenAccountsByOwnerRequest::new_with_config(*owner, token_account_filter, config);
        let response: GetTokenAccountsByOwnerResponse = self.send(request, opts).await?;

        Ok(response.value)
    }

    pub async fn get_token_accounts_by_owner(
        &self,
        owner: &Pubkey,
        token_account_filter: RpcTokenAccountsFilter,
        opts: CallOptions,
    ) -> ClientResult<Vec<RpcKeyedAccount>> {
        self.get_token_accounts_by_owner_with_commitment(
            owner,
            token_account_filter,
            self.commitment_config(),
            opts,
        )
        .await
    }

    pub async fn get_token_account_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<UiTokenAmount> {
        let request = GetTokenAccountBalanceRequest::new_with_config(*pubkey, commitment_config);
        let response: GetTokenAccountBalanceResponse = self.send(request, opts).await?;

        Ok(response.value)
    }

    pub async fn get_token_account_balance(
        &self,
        pubkey: &Pubkey,
        opts: CallOptions,
    ) -> ClientResult<UiTokenAmount> {
        self.get_token_account_balance_with_commitment(pubkey, self.commitment_config(), opts)
            .await
    }

    pub async fn get_token_supply_with_commitment(
        &self,
        mint: &Pubkey,
        commitment_config: CommitmentConfig,
        opts: CallOptions,
    ) -> ClientResult<UiTokenAmount> {
        let request = GetTokenSupplyRequest::new_with_config(*mint, commitment_config);
        let response: GetTokenSupplyResponse = self.send(request, opts).await?;

        Ok(response.value)
    }

    pub async fn get_token_supply(
        &self,
        mint: &Pubkey,
        opts: CallOptions,
    ) -> ClientResult<UiTokenAmount> {
        self.get_token_supply_with_commitment(mint, self.commitment_config(), opts)
            .await
    }

    pub async fn simulate_transaction_with_config(
        &self,
        transaction: &Transaction,
        config: RpcSimulateTransactionConfig,
        opts: CallOptions,
    ) -> ClientResult<SimulateTransactionResponse> {
        let request = SimulateTransactionRequest::new_with_config(transaction.to_owned(), config);
        let response: SimulateTransactionResponse = self.send(request, opts).await?;
        Ok(response)
    }

    pub async fn simulate_transaction(
        &self,
        transaction: &Transaction,
        opts: CallOptions,
    ) -> ClientResult<SimulateTransactionResponse> {
        self.simulate_transaction_with_config(
            transaction,
            RpcSimulateTransactionConfig {
                encoding: Some(UiTransactionEncoding::Base64),
                replace_recent_blockhash: Some(true),
                ..Default::default()
            },
            opts,
        )
        .await
    }
}
