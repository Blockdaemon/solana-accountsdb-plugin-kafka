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
        BlockEvent, Config, MessageWrapper, SlotStatusEvent, TransactionEvent, UpdateAccountEvent,
        message_wrapper::EventMessage::{self, Account, Slot, Transaction},
        prom::{
            StatsThreadedProducerContext, UPLOAD_ACCOUNTS_TOTAL, UPLOAD_SLOTS_TOTAL,
            UPLOAD_TRANSACTIONS_TOTAL,
        },
    },
    prost::Message,
    rdkafka::{
        error::KafkaError,
        producer::{BaseRecord, Producer, ThreadedProducer},
    },
    std::time::Duration,
};

pub struct Publisher {
    producer: ThreadedProducer<StatsThreadedProducerContext>,
    shutdown_timeout: Duration,
}

impl Publisher {
    pub fn new(producer: ThreadedProducer<StatsThreadedProducerContext>, config: &Config) -> Self {
        Self {
            producer,
            shutdown_timeout: Duration::from_millis(config.shutdown_timeout_ms),
        }
    }

    pub fn update_account(
        &self,
        ev: UpdateAccountEvent,
        wrap_messages: bool,
        topic: &str,
    ) -> Result<(), KafkaError> {
        let temp_key;
        let (key, buf) = if wrap_messages {
            (
                &ev.pubkey.clone(),
                Self::encode_with_wrapper(Account(Box::new(ev))),
            )
        } else {
            temp_key = self.copy_and_prepend(ev.pubkey.as_slice(), b'A');
            (&temp_key, ev.encode_to_vec())
        };
        let record = BaseRecord::<Vec<u8>, _>::to(topic).key(key).payload(&buf);
        let result = self.producer.send(record).map(|_| ()).map_err(|(e, _)| e);
        UPLOAD_ACCOUNTS_TOTAL
            .with_label_values(&[if result.is_ok() { "success" } else { "failed" }])
            .inc();
        result
    }

    pub fn update_slot_status(
        &self,
        ev: SlotStatusEvent,
        wrap_messages: bool,
        topic: &str,
    ) -> Result<(), KafkaError> {
        let temp_key;
        let (key, buf) = if wrap_messages {
            temp_key = ev.slot.to_le_bytes().to_vec();
            (&temp_key, Self::encode_with_wrapper(Slot(Box::new(ev))))
        } else {
            temp_key = self.copy_and_prepend(&ev.slot.to_le_bytes(), b'S');
            (&temp_key, ev.encode_to_vec())
        };
        let record = BaseRecord::<Vec<u8>, _>::to(topic).key(key).payload(&buf);
        let result = self.producer.send(record).map(|_| ()).map_err(|(e, _)| e);
        UPLOAD_SLOTS_TOTAL
            .with_label_values(&[if result.is_ok() { "success" } else { "failed" }])
            .inc();
        result
    }

    pub fn update_transaction(
        &self,
        ev: TransactionEvent,
        wrap_messages: bool,
        topic: &str,
    ) -> Result<(), KafkaError> {
        let temp_key;
        let (key, buf) = if wrap_messages {
            (
                &ev.signature.clone(),
                Self::encode_with_wrapper(Transaction(Box::new(ev))),
            )
        } else {
            temp_key = self.copy_and_prepend(ev.signature.as_slice(), b'T');
            (&temp_key, ev.encode_to_vec())
        };
        let record = BaseRecord::<Vec<u8>, _>::to(topic).key(key).payload(&buf);
        let result = self.producer.send(record).map(|_| ()).map_err(|(e, _)| e);
        UPLOAD_TRANSACTIONS_TOTAL
            .with_label_values(&[if result.is_ok() { "success" } else { "failed" }])
            .inc();
        result
    }
    pub fn update_block(
        &self,
        ev: BlockEvent,
        wrap_messages: bool,
        topic: &str,
    ) -> Result<(), KafkaError> {
        let temp_key;
        let (key, buf) = if wrap_messages {
            temp_key = ev.blockhash.as_bytes().to_vec();
            (
                &temp_key,
                Self::encode_with_wrapper(EventMessage::Block(Box::new(ev))),
            )
        } else {
            temp_key = self.copy_and_prepend(ev.blockhash.as_bytes(), b'B');
            (&temp_key, ev.encode_to_vec())
        };
        let record = BaseRecord::<Vec<u8>, _>::to(topic).key(key).payload(&buf);
        self.producer.send(record).map(|_| ()).map_err(|(e, _)| e)
    }

    fn encode_with_wrapper(message: EventMessage) -> Vec<u8> {
        MessageWrapper {
            event_message: Some(message),
        }
        .encode_to_vec()
    }

    fn copy_and_prepend(&self, data: &[u8], prefix: u8) -> Vec<u8> {
        let mut temp_key = Vec::with_capacity(data.len() + 1);
        temp_key.push(prefix);
        temp_key.extend_from_slice(data);
        temp_key
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        let _ = self.producer.flush(self.shutdown_timeout);
    }
}
