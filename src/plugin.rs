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
    crate::*,
    log::info,
    rdkafka::util::get_rdkafka_version,
    simple_error::simple_error,
    solana_geyser_plugin_interface::geyser_plugin_interface::{
        GeyserPlugin, GeyserPluginError as PluginError, ReplicaAccountInfo,
        ReplicaAccountInfoVersions, ReplicaTransactionInfo, ReplicaTransactionInfoVersions,
        Result as PluginResult, SlotStatus as PluginSlotStatus,
    },
    std::fmt::{Debug, Formatter},
};

#[derive(Default)]
pub struct KafkaPlugin {
    publisher: Option<Publisher>,
    filter: Option<Filter>,
    publish_all_accounts: bool,
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

    fn on_load(&mut self, config_file: &str) -> PluginResult<()> {
        if self.publisher.is_some() {
            let err = simple_error!("plugin already loaded");
            return Err(PluginError::Custom(Box::new(err)));
        }

        solana_logger::setup_with_default("info");
        info!(
            "Loading plugin {:?} from config_file {:?}",
            self.name(),
            config_file
        );
        let config = Config::read_from(config_file)?;
        self.publish_all_accounts = config.publish_all_accounts;

        let (version_n, version_s) = get_rdkafka_version();
        info!("rd_kafka_version: {:#08x}, {}", version_n, version_s);

        let producer = config
            .producer()
            .map_err(|e| PluginError::Custom(Box::new(e)))?;
        info!("Created rdkafka::FutureProducer");

        let publisher = Publisher::new(producer, &config);
        self.publisher = Some(publisher);
        self.filter = Some(Filter::new(&config));
        info!("Spawned producer");

        Ok(())
    }

    fn on_unload(&mut self) {
        self.publisher = None;
        self.filter = None;
    }

    fn update_account(
        &mut self,
        account: ReplicaAccountInfoVersions,
        slot: u64,
        is_startup: bool,
    ) -> PluginResult<()> {
        if is_startup && !self.publish_all_accounts {
            return Ok(());
        }

        let info = Self::unwrap_update_account(account);
        if !self.unwrap_filter().wants_program(info.owner) {
            return Ok(());
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
        };

        let publisher = self.unwrap_publisher();
        publisher
            .update_account(event)
            .map_err(|e| PluginError::AccountsUpdateError { msg: e.to_string() })
    }

    fn update_slot_status(
        &mut self,
        slot: u64,
        parent: Option<u64>,
        status: PluginSlotStatus,
    ) -> PluginResult<()> {
        let publisher = self.unwrap_publisher();
        if !publisher.wants_slot_status() {
            return Ok(());
        }

        let event = SlotStatusEvent {
            slot,
            parent: parent.unwrap_or(0),
            status: SlotStatus::from(status).into(),
        };

        publisher
            .update_slot_status(event)
            .map_err(|e| PluginError::AccountsUpdateError { msg: e.to_string() })
    }

    fn notify_transaction(
        &mut self,
        transaction: ReplicaTransactionInfoVersions,
        slot: u64,
    ) -> PluginResult<()> {
        let publisher = self.unwrap_publisher();
        if !publisher.wants_transaction() {
            return Ok(());
        }

        let filter = self.unwrap_filter();
        let transaction = Self::unwrap_notify_transaction(transaction);
        if !transaction
            .transaction
            .message()
            .account_keys_iter()
            .any(|pubkey| filter.wants_program(pubkey.as_ref()))
        {
            return Ok(());
        }

        let event = Self::build_transaction_event(slot, transaction);

        publisher
            .update_transaction(event)
            .map_err(|e| PluginError::TransactionUpdateError { msg: e.to_string() })
    }

    fn account_data_notifications_enabled(&self) -> bool {
        self.unwrap_publisher().wants_update_account()
    }

    fn transaction_notifications_enabled(&self) -> bool {
        self.unwrap_publisher().wants_transaction()
    }
}

impl KafkaPlugin {
    pub fn new() -> Self {
        Default::default()
    }

    fn unwrap_publisher(&self) -> &Publisher {
        self.publisher.as_ref().expect("publisher is unavailable")
    }

    fn unwrap_filter(&self) -> &Filter {
        self.filter.as_ref().expect("filter is unavailable")
    }

    fn unwrap_update_account(account: ReplicaAccountInfoVersions) -> &ReplicaAccountInfo {
        match account {
            ReplicaAccountInfoVersions::V0_0_1(info) => info,
        }
    }

    fn unwrap_notify_transaction(
        transaction: ReplicaTransactionInfoVersions,
    ) -> &ReplicaTransactionInfo {
        match transaction {
            ReplicaTransactionInfoVersions::V0_0_1(info) => info,
        }
    }

    fn build_compiled_instruction(
        ix: &solana_program::instruction::CompiledInstruction,
    ) -> CompiledInstruction {
        CompiledInstruction {
            program_id_index: ix.program_id_index as u32,
            accounts: ix.clone().accounts.into_iter().map(|v| v as u32).collect(),
            data: ix.data.clone(),
        }
    }

    fn build_message_header(header: &solana_program::message::MessageHeader) -> MessageHeader {
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
        transaction: &ReplicaTransactionInfo,
    ) -> TransactionEvent {
        let transaction_status_meta = transaction.transaction_status_meta;
        let signature = transaction.signature;
        let is_vote = transaction.is_vote;
        let transaction = transaction.transaction;
        TransactionEvent {
            is_vote,
            slot,
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
                    None => vec![],
                    Some(inners) => inners
                        .clone()
                        .into_iter()
                        .map(|inner| InnerInstruction {
                            index: inner.index as u32,
                            instructions: inner
                                .instructions
                                .iter()
                                .map(Self::build_compiled_instruction)
                                .collect(),
                        })
                        .collect(),
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
            }),
            transaction: Some(SanitizedTransaction {
                message_hash: transaction.message_hash().to_bytes().into(),
                is_simple_vote_transaction: transaction.is_simple_vote_transaction(),
                message: Some(SanitizedMessage {
                    message_payload: Some(match transaction.message() {
                        solana_program::message::SanitizedMessage::Legacy(lv) => {
                            sanitized_message::MessagePayload::Legacy(LegacyMessage {
                                header: Some(Self::build_message_header(&lv.header)),
                                account_keys: lv
                                    .account_keys
                                    .clone()
                                    .into_iter()
                                    .map(|k| k.as_ref().into())
                                    .collect(),
                                instructions: lv
                                    .instructions
                                    .iter()
                                    .map(Self::build_compiled_instruction)
                                    .collect(),
                                recent_block_hash: lv.recent_blockhash.as_ref().into(),
                            })
                        }
                        solana_program::message::SanitizedMessage::V0(v0) => {
                            sanitized_message::MessagePayload::V0(V0LoadedMessage {
                                message: Some(V0Message {
                                    header: Some(Self::build_message_header(&v0.message.header)),
                                    account_keys: v0
                                        .message
                                        .account_keys
                                        .clone()
                                        .into_iter()
                                        .map(|k| k.as_ref().into())
                                        .collect(),
                                    recent_block_hash: v0.message.recent_blockhash.as_ref().into(),
                                    instructions: v0
                                        .message
                                        .instructions
                                        .iter()
                                        .map(Self::build_compiled_instruction)
                                        .collect(),
                                    address_table_lookup: v0
                                        .message
                                        .address_table_lookups
                                        .clone()
                                        .into_iter()
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
                                        .loaded_addresses
                                        .writable
                                        .clone()
                                        .into_iter()
                                        .map(|x| x.as_ref().into())
                                        .collect(),
                                    readonly: v0
                                        .loaded_addresses
                                        .readonly
                                        .clone()
                                        .into_iter()
                                        .map(|x| x.as_ref().into())
                                        .collect(),
                                }),
                            })
                        }
                    }),
                }),
                signatures: transaction
                    .signatures()
                    .iter()
                    .copied()
                    .map(|x| x.as_ref().into())
                    .collect(),
            }),
        }
    }
}
