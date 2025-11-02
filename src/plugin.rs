// Copyright 2022 Blockdaemon Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use {
    crate::{
        sanitized_message, BlockEvent, CompiledInstruction, Config, Filter, InnerInstruction,
        InnerInstructions, LegacyLoadedMessage, LegacyMessage, LoadedAddresses,
        MessageAddressTableLookup, MessageHeader, PrometheusService, Publisher, Reward,
        RewardsAndNumPartitions, SanitizedMessage, SanitizedTransaction, SlotStatus,
        SlotStatusEvent, TransactionEvent, TransactionStatusMeta, TransactionTokenBalance,
        UiTokenAmount, UpdateAccountEvent, V0LoadedMessage, V0Message,
    },
    agave_geyser_plugin_interface::geyser_plugin_interface::{
        GeyserPlugin, GeyserPluginError as PluginError, ReplicaAccountInfoV3,
        ReplicaAccountInfoVersions, ReplicaBlockInfoV4, ReplicaBlockInfoVersions,
        ReplicaTransactionInfoV3, ReplicaTransactionInfoVersions, Result as PluginResult,
        SlotStatus as PluginSlotStatus,
    },
    base58::FromBase58,
    log::{debug, error, info, log_enabled},
    rdkafka::util::get_rdkafka_version,
    solana_pubkey::{pubkey, Pubkey},
    std::fmt::{Debug, Formatter},
};

#[derive(Default)]
pub struct KafkaPlugin {
    publisher: Option<Publisher>,
    filter: Option<Vec<Filter>>,
    block_event_topic: Option<String>,
    prometheus: Option<PrometheusService>,
}

impl Debug for KafkaPlugin {
    fn fmt(&self, _: &mut Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl GeyserPlugin for KafkaPlugin {
    fn name(&self) -> &'static str {
        "KafkaPlugin"
    }

    fn on_load(&mut self, config_file: &str, _: bool) -> PluginResult<()> {
        if self.publisher.is_some() {
            return Err(PluginError::Custom("plugin already loaded".into()));
        }

        solana_logger::setup_with_default("info");
        info!(
            "Loading plugin {:?} from config_file {:?}",
            self.name(),
            config_file
        );
        let config = Config::read_from(config_file)?;

        let (version_n, version_s) = get_rdkafka_version();
        info!("rd_kafka_version: {:#08x}, {}", version_n, version_s);

        let producer = config.producer().map_err(|error| {
            error!("Failed to create kafka producer: {error:?}");
            PluginError::Custom(Box::new(error))
        })?;
        info!("Created rdkafka::FutureProducer");

        let publisher = Publisher::new(producer, &config);
        let prometheus = config
            .create_prometheus()
            .map_err(|error| PluginError::Custom(Box::new(error)))?;
        self.publisher = Some(publisher);
        self.filter = Some(config.filters.iter().map(Filter::new).collect());
        self.prometheus = prometheus;
        info!("Spawned producer");

        Ok(())
    }

    fn on_unload(&mut self) {
        self.publisher = None;
        self.filter = None;
        if let Some(prometheus) = self.prometheus.take() {
            prometheus.shutdown();
        }
    }

    fn update_account(
        &self,
        account: ReplicaAccountInfoVersions,
        slot: u64,
        is_startup: bool,
    ) -> PluginResult<()> {
        let filters = self.unwrap_filters();
        if is_startup && filters.iter().all(|filter| !filter.publish_all_accounts) {
            return Ok(());
        }

        let info = Self::unwrap_update_account(account);
        let publisher = self.unwrap_publisher();
        for filter in filters {
            if !filter.update_account_topic.is_empty() {
                if !filter.wants_program(info.owner) && !filter.wants_account(info.pubkey) {
                    Self::log_ignore_account_update(info);
                    continue;
                }

                let event = UpdateAccountEvent {
                    slot,
                    pubkey: info.pubkey.to_vec(),
                    lamports: info.lamports,
                    owner: info.owner.to_vec(),
                    executable: info.executable,
                    rent_epoch: info.rent_epoch,
                    data: info.data.to_vec(),
                    write_version: info.write_version,
                    txn_signature: info.txn.map(|v| v.signature().as_ref().to_owned()),
                    data_version: info.write_version as u32, // Use write_version as data version
                    is_startup,                              // Use the is_startup parameter
                    account_age: slot.saturating_sub(info.rent_epoch), // Approximate age from rent epoch
                };

                publisher
                    .update_account(event, filter.wrap_messages, &filter.update_account_topic)
                    .map_err(|e| PluginError::AccountsUpdateError { msg: e.to_string() })?;
            }
        }

        Ok(())
    }

    fn update_slot_status(
        &self,
        slot: u64,
        parent: Option<u64>,
        status: &PluginSlotStatus,
    ) -> PluginResult<()> {
        let publisher = self.unwrap_publisher();
        let value = SlotStatus::from(status.clone());
        for filter in self.unwrap_filters() {
            if !filter.slot_status_topic.is_empty() {
                let event = SlotStatusEvent {
                    slot,
                    parent: parent.unwrap_or(0),
                    status: value.into(),
                    is_confirmed: Self::is_slot_confirmed(&value), // Derived from status
                    confirmation_count: Self::calculate_confirmation_count(&value), // Calculate from status
                    status_description: Self::get_slot_status_description(&value), // Get human-readable status
                };

                publisher
                    .update_slot_status(event, filter.wrap_messages, &filter.slot_status_topic)
                    .map_err(|e| PluginError::AccountsUpdateError { msg: e.to_string() })?;
            }
        }

        Ok(())
    }

    fn notify_transaction(
        &self,
        transaction: ReplicaTransactionInfoVersions,
        slot: u64,
    ) -> PluginResult<()> {
        let info = Self::unwrap_transaction(transaction);
        let publisher = self.unwrap_publisher();
        for filter in self.unwrap_filters() {
            if !filter.transaction_topic.is_empty() {
                let is_failed = info.transaction_status_meta.status.is_err();
                if (!filter.wants_vote_tx() && info.is_vote)
                    || (!filter.wants_failed_tx() && is_failed)
                {
                    debug!("Ignoring vote/failed transaction");
                    continue;
                }

                if !info
                    .transaction
                    .message
                    .static_account_keys()
                    .iter()
                    .any(|pubkey| {
                        filter.wants_program(pubkey.as_ref())
                            || filter.wants_account(pubkey.as_ref())
                    })
                {
                    debug!("Ignoring transaction {:?}", info.signature);
                    continue;
                }

                let event = Self::build_transaction_event(slot, info);
                publisher
                    .update_transaction(event, filter.wrap_messages, &filter.transaction_topic)
                    .map_err(|e| PluginError::TransactionUpdateError { msg: e.to_string() })?;
            }
        }

        Ok(())
    }
    fn notify_block_metadata(&self, blockinfo: ReplicaBlockInfoVersions) -> PluginResult<()> {
        let Some(topic) = &self.block_event_topic else {
            return Ok(());
        };
        let info = Self::unwrap_block_metadata(blockinfo);
        let publisher = self.unwrap_publisher();
        let event = Self::build_block_event(info.clone());
        publisher.update_block(event, true, topic).unwrap();
        Ok(())
    }

    fn account_data_notifications_enabled(&self) -> bool {
        let filters = self.unwrap_filters();
        filters
            .iter()
            .any(|filter| !filter.update_account_topic.is_empty())
    }

    fn transaction_notifications_enabled(&self) -> bool {
        let filters = self.unwrap_filters();
        filters
            .iter()
            .any(|filter| !filter.transaction_topic.is_empty())
    }
}

impl KafkaPlugin {
    /// Compute Budget Program ID
    const COMPUTE_BUDGET_PROGRAM_ID: Pubkey =
        pubkey!("ComputeBudget111111111111111111111111111111");

    pub fn new() -> Self {
        Default::default()
    }

    fn unwrap_publisher(&self) -> &Publisher {
        self.publisher.as_ref().expect("publisher is unavailable")
    }

    fn unwrap_filters(&self) -> &Vec<Filter> {
        self.filter.as_ref().expect("filter is unavailable")
    }

    fn unwrap_update_account(account: ReplicaAccountInfoVersions) -> &ReplicaAccountInfoV3 {
        match account {
            ReplicaAccountInfoVersions::V0_0_1(_info) => {
                panic!("ReplicaAccountInfoVersions::V0_0_1 unsupported, please upgrade your Solana node.");
            }
            ReplicaAccountInfoVersions::V0_0_2(_info) => {
                panic!("ReplicaAccountInfoVersions::V0_0_2 unsupported, please upgrade your Solana node.");
            }
            ReplicaAccountInfoVersions::V0_0_3(info) => info,
        }
    }

    fn unwrap_transaction(
        transaction: ReplicaTransactionInfoVersions,
    ) -> &ReplicaTransactionInfoV3 {
        match transaction {
            ReplicaTransactionInfoVersions::V0_0_1(_info) => {
                panic!("ReplicaTransactionInfoVersions::V0_0_1 unsupported, please upgrade your Solana node.");
            }
            ReplicaTransactionInfoVersions::V0_0_2(_info) => {
                panic!("ReplicaAccountInfoVersions::V0_0_2 unsupported, please upgrade your Solana node.");
            }
            ReplicaTransactionInfoVersions::V0_0_3(info) => info,
        }
    }
    fn unwrap_block_metadata(block: ReplicaBlockInfoVersions) -> &ReplicaBlockInfoV4 {
        match block {
            ReplicaBlockInfoVersions::V0_0_1(_info) => {
                panic!("ReplicaBlockInfoVersions::V0_0_1 unsupported, please upgrade your Solana node.");
            }
            ReplicaBlockInfoVersions::V0_0_2(_info) => {
                panic!("ReplicaBlockInfoVersions::V0_0_2 unsupported, please upgrade your Solana node.");
            }
            ReplicaBlockInfoVersions::V0_0_3(_info) => {
                panic!("ReplicaBlockInfoVersions::V0_0_3 unsupported, please upgrade your Solana node.");
            }
            ReplicaBlockInfoVersions::V0_0_4(info) => info,
        }
    }

    fn build_compiled_instruction(
        ix: &solana_message::compiled_instruction::CompiledInstruction,
    ) -> CompiledInstruction {
        CompiledInstruction {
            program_id_index: ix.program_id_index as u32,
            accounts: ix.clone().accounts.into_iter().map(|v| v as u32).collect(),
            data: ix.data.clone(),
        }
    }

    fn build_inner_instruction(
        ix: &solana_transaction_status::InnerInstruction,
    ) -> InnerInstruction {
        InnerInstruction {
            instruction: Some(Self::build_compiled_instruction(&ix.instruction)),
            stack_height: ix.stack_height,
        }
    }

    fn build_message_header(header: &solana_message::MessageHeader) -> MessageHeader {
        MessageHeader {
            num_required_signatures: header.num_required_signatures as u32,
            num_readonly_signed_accounts: header.num_readonly_signed_accounts as u32,
            num_readonly_unsigned_accounts: header.num_readonly_unsigned_accounts as u32,
        }
    }

    fn build_transaction_token_balance(
        transaction_token_account_balance: solana_transaction_status::TransactionTokenBalance,
    ) -> TransactionTokenBalance {
        TransactionTokenBalance {
            account_index: transaction_token_account_balance.account_index as u32,
            ui_token_account: Some(UiTokenAmount {
                ui_amount: transaction_token_account_balance.ui_token_amount.ui_amount,
                decimals: transaction_token_account_balance.ui_token_amount.decimals as u32,
                amount: transaction_token_account_balance.ui_token_amount.amount,
                ui_amount_string: transaction_token_account_balance
                    .ui_token_amount
                    .ui_amount_string,
            }),
            mint: transaction_token_account_balance.mint,
            owner: transaction_token_account_balance.owner,
        }
    }

    fn build_transaction_event(
        slot: u64,
        ReplicaTransactionInfoV3 {
            signature,
            is_vote,
            transaction,
            transaction_status_meta,
            index,
            message_hash,
        }: &ReplicaTransactionInfoV3,
    ) -> TransactionEvent {
        TransactionEvent {
            is_vote: *is_vote,
            slot,
            index: *index as u64,
            signature: signature.as_ref().into(),
            transaction_status_meta: Some(TransactionStatusMeta {
                is_status_err: transaction_status_meta.status.is_err(),
                error_info: match &transaction_status_meta.status {
                    Err(e) => e.to_string(),
                    Ok(_) => "".to_owned(),
                },
                rewards: transaction_status_meta
                    .rewards
                    .clone()
                    .unwrap()
                    .into_iter()
                    .map(|x| Reward {
                        pubkey: x.pubkey,
                        lamports: x.lamports,
                        post_balance: x.post_balance,
                        reward_type: match x.reward_type {
                            Some(r) => r as i32,
                            None => 0,
                        },
                        commission: match x.commission {
                            Some(v) => v as u32,
                            None => 0,
                        },
                    })
                    .collect(),
                fee: transaction_status_meta.fee,
                log_messages: match &transaction_status_meta.log_messages {
                    Some(v) => v.to_owned(),
                    None => vec![],
                },
                inner_instructions: match &transaction_status_meta.inner_instructions {
                    Some(inners) => inners
                        .clone()
                        .into_iter()
                        .map(|inner| InnerInstructions {
                            index: inner.index as u32,
                            instructions: inner
                                .instructions
                                .iter()
                                .map(Self::build_inner_instruction)
                                .collect(),
                        })
                        .collect(),
                    None => vec![],
                },
                pre_balances: transaction_status_meta.pre_balances.clone(),
                post_balances: transaction_status_meta.post_balances.clone(),
                pre_token_balances: match &transaction_status_meta.pre_token_balances {
                    Some(v) => v
                        .clone()
                        .into_iter()
                        .map(Self::build_transaction_token_balance)
                        .collect(),
                    None => vec![],
                },
                post_token_balances: match &transaction_status_meta.post_token_balances {
                    Some(v) => v
                        .clone()
                        .into_iter()
                        .map(Self::build_transaction_token_balance)
                        .collect(),
                    None => vec![],
                },
                compute_units_consumed: Self::extract_compute_units_from_metadata(
                    transaction_status_meta,
                ),
                compute_units_price: Self::extract_compute_price_from_transaction(
                    &transaction.message,
                ),
                error_logs: Self::extract_error_logs_from_status(&transaction_status_meta.status),
                is_successful: transaction_status_meta.status.is_ok(), // Derived from status
            }),
            transaction: Some(SanitizedTransaction {
                message_hash: message_hash.to_bytes().into(),
                is_simple_vote_transaction: *is_vote,
                message: Some(SanitizedMessage {
                    message_payload: Some(match &transaction.message {
                        solana_message::VersionedMessage::Legacy(lv) => {
                            // Use LegacyLoadedMessage for Legacy messages
                            sanitized_message::MessagePayload::Legacy(LegacyLoadedMessage {
                                message: Some(LegacyMessage {
                                    header: Some(Self::build_message_header(&lv.header)),
                                    account_keys: lv
                                        .account_keys
                                        .iter()
                                        .map(|k| k.as_ref().into())
                                        .collect(),
                                    recent_block_hash: lv.recent_blockhash.as_ref().into(),
                                    instructions: lv
                                        .instructions
                                        .iter()
                                        .map(Self::build_compiled_instruction)
                                        .collect(),
                                }),
                                is_writable_account_cache: {
                                    // Derive writable status from message header and account positions
                                    let num_required = lv.header.num_required_signatures as usize;
                                    let num_readonly_signed =
                                        lv.header.num_readonly_signed_accounts as usize;

                                    (0..lv.account_keys.len())
                                        .map(|i| {
                                            if i < num_required {
                                                true // Required signers are always writable
                                            } else if i < num_required + num_readonly_signed {
                                                false // Readonly signed accounts
                                            } else {
                                                true // Remaining accounts are writable
                                            }
                                        })
                                        .collect()
                                },
                            })
                        }
                        solana_message::VersionedMessage::V0(v0) => {
                            // Use V0LoadedMessage for V0 messages
                            sanitized_message::MessagePayload::V0(V0LoadedMessage {
                                message: Some(V0Message {
                                    header: Some(Self::build_message_header(&v0.header)),
                                    account_keys: v0
                                        .account_keys
                                        .iter()
                                        .map(|k| k.as_ref().into())
                                        .collect(),
                                    recent_block_hash: v0.recent_blockhash.as_ref().into(),
                                    instructions: v0
                                        .instructions
                                        .iter()
                                        .map(Self::build_compiled_instruction)
                                        .collect(),
                                    address_table_lookup: v0
                                        .address_table_lookups
                                        .iter()
                                        .map(|vf| MessageAddressTableLookup {
                                            account_key: vf.account_key.as_ref().into(),
                                            writable_indexes: vf
                                                .writable_indexes
                                                .iter()
                                                .map(|x| *x as u32)
                                                .collect(),
                                            readonly_indexes: vf
                                                .readonly_indexes
                                                .iter()
                                                .map(|x| *x as u32)
                                                .collect(),
                                        })
                                        .collect(),
                                }),
                                loaded_adresses: Some(LoadedAddresses {
                                    writable: v0
                                        .address_table_lookups
                                        .iter()
                                        .flat_map(|lookup| {
                                            lookup.writable_indexes.iter().map(|&_idx| {
                                                vec![0u8; 32] // Placeholder - actual keys not available
                                            })
                                        })
                                        .collect(),
                                    readonly: v0
                                        .address_table_lookups
                                        .iter()
                                        .flat_map(|lookup| {
                                            lookup.readonly_indexes.iter().map(|&_idx| {
                                                vec![0u8; 32] // Placeholder - actual keys not available
                                            })
                                        })
                                        .collect(),
                                    writable_info: Self::build_loaded_address_info(
                                        &v0.address_table_lookups,
                                        &v0.account_keys,
                                        true,
                                    ),
                                    readonly_info: Self::build_loaded_address_info(
                                        &v0.address_table_lookups,
                                        &v0.account_keys,
                                        false,
                                    ),
                                }),
                                is_writable_account_cache: {
                                    // Derive writable status from message header and account positions
                                    let num_required = v0.header.num_required_signatures as usize;
                                    let num_readonly_signed =
                                        v0.header.num_readonly_signed_accounts as usize;

                                    (0..v0.account_keys.len())
                                        .map(|i| {
                                            if i < num_required {
                                                true // Required signers are always writable
                                            } else if i < num_required + num_readonly_signed {
                                                false // Readonly signed accounts
                                            } else {
                                                true // Remaining accounts are writable
                                            }
                                        })
                                        .collect()
                                },
                            })
                        }
                    }),
                }),
                signatures: transaction
                    .signatures
                    .iter()
                    .copied()
                    .map(|x| x.as_ref().into())
                    .collect(),
            }),
            compute_units_consumed: Self::extract_compute_units_from_metadata(
                transaction_status_meta,
            ),
            compute_units_price: Self::extract_compute_price_from_transaction(&transaction.message),
            total_cost: transaction_status_meta.fee
                + Self::extract_compute_price_from_transaction(&transaction.message),
            instruction_count: transaction.message.instructions().len() as u32,
            account_count: Self::get_account_keys_from_message(&transaction.message)
                .map(|keys| keys.len() as u32)
                .unwrap_or(0),
            execution_time_ns: 0, // Not available in current API
            is_successful: transaction_status_meta.status.is_ok(),
            execution_logs: transaction_status_meta
                .log_messages
                .clone()
                .unwrap_or_default(),
            error_details: Self::extract_error_logs_from_status(&transaction_status_meta.status),
            confirmation_count: 0, // Will be populated from slot status when available
        }
    }

    fn log_ignore_account_update(info: &ReplicaAccountInfoV3) {
        if log_enabled!(::log::Level::Debug) {
            match <&[u8; 32]>::try_from(info.owner) {
                Ok(key) => debug!(
                    "Ignoring update for account key: {:?}",
                    Pubkey::new_from_array(*key)
                ),
                // Err should never happen because wants_account_key only returns false if the input is &[u8; 32]
                Err(_err) => debug!("Ignoring update for account key: {:?}", info.owner),
            };
        }
    }

    /// Extract compute units consumed from transaction metadata
    fn extract_compute_units_from_metadata(
        transaction_status_meta: &solana_transaction_status::TransactionStatusMeta,
    ) -> u32 {
        // Check if compute units are available in the metadata
        if let Some(compute_units) = transaction_status_meta.compute_units_consumed {
            compute_units as u32
        } else {
            // If not available in metadata, return 0
            // We avoid log parsing as it's unreliable
            0
        }
    }

    /// Extract compute unit price from transaction message
    fn extract_compute_price_from_transaction(message: &solana_message::VersionedMessage) -> u64 {
        // Look for compute budget instructions in the transaction
        let instructions = message.instructions();

        for instruction in instructions {
            // Check if this is a compute budget instruction
            let program_id_index = instruction.program_id_index as usize;
            if let Some(account_keys) = Self::get_account_keys_from_message(message) {
                if program_id_index < account_keys.len() {
                    let program_id = &account_keys[program_id_index];

                    if *program_id == Self::COMPUTE_BUDGET_PROGRAM_ID {
                        // Parse compute budget instruction data to extract price
                        let data = &instruction.data;
                        if data.len() >= 9 && data[0] == 3 {
                            // SetComputeUnitPrice instruction (discriminator 3)
                            let price = u64::from_le_bytes([
                                data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                                data[8],
                            ]);
                            return price;
                        }
                    }
                }
            }
        }
        0 // Default price if not found
    }

    /// Extract account keys from versioned message
    fn get_account_keys_from_message(
        message: &solana_message::VersionedMessage,
    ) -> Option<&[solana_pubkey::Pubkey]> {
        match message {
            solana_message::VersionedMessage::Legacy(lv) => Some(&lv.account_keys),
            solana_message::VersionedMessage::V0(v0) => Some(&v0.account_keys),
        }
    }

    /// Extract error information from transaction status (more reliable than log parsing)
    fn extract_error_logs_from_status<T: std::fmt::Display>(status: &Result<(), T>) -> Vec<String> {
        match status {
            Ok(_) => vec![], // No errors
            Err(error) => {
                // Convert the actual error to a string representation
                vec![error.to_string()]
            }
        }
    }

    /// Determine if slot is confirmed based on status
    fn is_slot_confirmed(status: &SlotStatus) -> bool {
        matches!(status, SlotStatus::Confirmed | SlotStatus::Rooted)
    }

    /// Get human-readable slot status description
    fn get_slot_status_description(status: &SlotStatus) -> String {
        match status {
            SlotStatus::Processed => "Processed - highest slot of heaviest fork".to_string(),
            SlotStatus::Rooted => {
                "Rooted - highest slot having reached max vote lockout".to_string()
            }
            SlotStatus::Confirmed => "Confirmed - voted on by supermajority of cluster".to_string(),
            SlotStatus::FirstShredReceived => "First shred received".to_string(),
            SlotStatus::Completed => "Completed".to_string(),
            SlotStatus::CreatedBank => "Created bank".to_string(),
            SlotStatus::Dead => "Dead - fork has been abandoned".to_string(),
        }
    }

    /// Build detailed loaded address information
    fn build_loaded_address_info(
        _address_table_lookups: &[solana_message::v0::MessageAddressTableLookup],
        _account_keys: &[solana_pubkey::Pubkey],
        is_writable: bool,
    ) -> Vec<crate::LoadedAddressInfo> {
        let mut address_info = Vec::new();

        for lookup in _address_table_lookups.iter() {
            let indexes = if is_writable {
                &lookup.writable_indexes
            } else {
                &lookup.readonly_indexes
            };

            for &index in indexes.iter() {
                // Create LoadedAddressInfo with available data
                let info = crate::LoadedAddressInfo {
                    address: lookup.account_key.as_ref().into(),
                    index: index as u32,
                    is_writable,
                };
                address_info.push(info);
            }
        }

        address_info
    }

    /// Calculate confirmation count based on slot status
    fn calculate_confirmation_count(status: &SlotStatus) -> u32 {
        match status {
            SlotStatus::Processed => 0,          // Not confirmed yet
            SlotStatus::Rooted => 2,             // Fully confirmed (rooted)
            SlotStatus::Confirmed => 1,          // Confirmed by supermajority
            SlotStatus::FirstShredReceived => 0, // Early stage
            SlotStatus::Completed => 1,          // Considered confirmed
            SlotStatus::CreatedBank => 0,        // Early stage
            SlotStatus::Dead => 0,               // Abandoned fork
        }
    }
    fn build_block_event(block: ReplicaBlockInfoV4) -> BlockEvent {
        let rewards = block
            .rewards
            .rewards
            .iter()
            .map(|x| Reward {
                pubkey: x.pubkey.clone(),
                lamports: x.lamports,
                post_balance: x.post_balance,
                reward_type: match x.reward_type {
                    Some(r) => r as i32,
                    None => 0,
                },
                commission: match x.commission {
                    Some(v) => v as u32,
                    None => 0,
                },
            })
            .collect();
        BlockEvent {
            parent_slot: block.parent_slot,
            parent_blockhash: block.parent_blockhash.from_base58().unwrap(),
            slot: block.slot,
            blockhash: block.blockhash.from_base58().unwrap(),
            rewards: Some(RewardsAndNumPartitions {
                rewards,
                num_partitions: block.rewards.num_partitions,
            }),
            block_time: block.block_time,
            block_height: block.block_height,
            executed_transaction_count: block.executed_transaction_count,
            entry_count: block.entry_count,
        }
    }
}
