use crate::message_wrapper::EventMessage;
use {
    crate::*,
    rdkafka::{
        error::KafkaError,
        producer::{BaseRecord, FutureProducer as Producer},
    },
    serde_json::to_vec,
    std::time::Duration,
};

pub struct Publisher {
    producer: Producer,
    shutdown_timeout: Duration,

    update_account_topic: String,
    slot_status_topic: String,
    transaction_topic: String,
}

impl Publisher {
    pub fn new(producer: Producer, config: &Config) -> Self {
        Self {
            producer,
            shutdown_timeout: Duration::from_millis(config.shutdown_timeout_ms),
            update_account_topic: config.update_account_topic.clone(),
            slot_status_topic: config.slot_status_topic.clone(),
            transaction_topic: config.transaction_topic.clone(),
        }
    }

    pub fn update_account(&self, ev: UpdateAccountEvent) -> Result<(), KafkaError> {
        let json = to_vec(&ev).map_err(|_| KafkaError::MessageProductionFailed("JSON Serialization failed"))?;
        let record = BaseRecord::to(&self.update_account_topic)
            .key(&ev.pubkey)
            .payload(json);
        self.producer.send(record).map_err(|(e, _)| e)
    }

    pub fn update_slot_status(&self, ev: SlotStatusEvent) -> Result<(), KafkaError> {
        let json = to_vec(&ev).map_err(|_| KafkaError::MessageProductionFailed("JSON Serialization failed"))?;
        let record = BaseRecord::to(&self.slot_status_topic)
            .key(ev.slot.to_le_bytes().to_vec())
            .payload(json);
        self.producer.send(record).map_err(|(e, _)| e)
    }

    pub fn update_transaction(&self, ev: TransactionEvent) -> Result<(), KafkaError> {
        let json = to_vec(&ev).map_err(|_| KafkaError::MessageProductionFailed("JSON Serialization failed"))?;
        let record = BaseRecord::to(&self.transaction_topic)
            .key(&ev.signature)
            .payload(json);
        self.producer.send(record).map_err(|(e, _)| e)
    }

    pub fn wants_update_account(&self) -> bool {
        !self.update_account_topic.is_empty()
    }

    pub fn wants_slot_status(&self) -> bool {
        !self.slot_status_topic.is_empty()
    }

    pub fn wants_transaction(&self) -> bool {
        !self.transaction_topic.is_empty()
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        let _ = self.producer.flush(self.shutdown_timeout.as_millis() as i32);
    }
}
