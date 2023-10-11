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
    crate::ConfigFilter,
    solana_program::pubkey::Pubkey,
    std::{collections::HashSet, str::FromStr},
};

pub struct Filter {
    pub publish_all_accounts: bool,
    pub program_ignores: HashSet<[u8; 32]>,
    pub program_filters: HashSet<[u8; 32]>,
    pub account_filters: HashSet<[u8; 32]>,
    pub include_vote_transactions: bool,
    pub include_failed_transactions: bool,

    pub update_account_topic: String,
    pub slot_status_topic: String,
    pub transaction_topic: String,

    pub wrap_messages: bool,
}

impl Filter {
    pub fn new(config: &ConfigFilter) -> Self {
        Self {
            publish_all_accounts: config.publish_all_accounts,
            program_ignores: config
                .program_ignores
                .iter()
                .flat_map(|p| Pubkey::from_str(p).ok().map(|p| p.to_bytes()))
                .collect(),
            program_filters: config
                .program_filters
                .iter()
                .flat_map(|p| Pubkey::from_str(p).ok().map(|p| p.to_bytes()))
                .collect(),
            account_filters: config
                .account_filters
                .iter()
                .flat_map(|p| Pubkey::from_str(p).ok().map(|p| p.to_bytes()))
                .collect(),
            include_vote_transactions: config.include_vote_transactions,
            include_failed_transactions: config.include_failed_transactions,

            update_account_topic: config.update_account_topic.clone(),
            slot_status_topic: config.slot_status_topic.clone(),
            transaction_topic: config.transaction_topic.clone(),

            wrap_messages: config.wrap_messages,
        }
    }

    pub fn wants_program(&self, program: &[u8]) -> bool {
        match <&[u8; 32]>::try_from(program) {
            Ok(key) => {
                !self.program_ignores.contains(key)
                    && (self.program_filters.is_empty() || self.program_filters.contains(key))
            }
            Err(_error) => true,
        }
    }

    pub fn wants_account(&self, account: &[u8]) -> bool {
        match <&[u8; 32]>::try_from(account) {
            Ok(key) => self.account_filters.contains(key),
            Err(_error) => true,
        }
    }

    pub fn wants_vote_tx(&self) -> bool {
        self.include_vote_transactions
    }

    pub fn wants_failed_tx(&self) -> bool {
        self.include_failed_transactions
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::{ConfigFilter, Filter},
        solana_program::pubkey::Pubkey,
        std::str::FromStr,
    };

    #[test]
    fn test_filter() {
        let config = ConfigFilter {
            program_ignores: vec![
                "Sysvar1111111111111111111111111111111111111".to_owned(),
                "Vote111111111111111111111111111111111111111".to_owned(),
            ],
            program_filters: vec!["9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin".to_owned()],
            ..Default::default()
        };

        let filter = Filter::new(&config);
        assert_eq!(filter.program_ignores.len(), 2);

        assert!(filter.wants_program(
            &Pubkey::from_str("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin")
                .unwrap()
                .to_bytes()
        ));
        assert!(!filter.wants_program(
            &Pubkey::from_str("Vote111111111111111111111111111111111111111")
                .unwrap()
                .to_bytes()
        ));
    }

    #[test]
    fn test_owner_filter() {
        let config = ConfigFilter {
            program_ignores: vec![
                "Sysvar1111111111111111111111111111111111111".to_owned(),
                "Vote111111111111111111111111111111111111111".to_owned(),
            ],
            program_filters: vec!["9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin".to_owned()],
            ..Default::default()
        };

        let filter = Filter::new(&config);
        assert_eq!(filter.program_ignores.len(), 2);

        assert!(filter.wants_program(
            &Pubkey::from_str("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin")
                .unwrap()
                .to_bytes()
        ));
        assert!(!filter.wants_program(
            &Pubkey::from_str("Vote111111111111111111111111111111111111111")
                .unwrap()
                .to_bytes()
        ));

        assert!(!filter.wants_program(
            &Pubkey::from_str("cndy3Z4yapfJBmL3ShUp5exZKqR3z33thTzeNMm2gRZ")
                .unwrap()
                .to_bytes()
        ));
    }

    #[test]
    fn test_account_filter() {
        let config = ConfigFilter {
            program_filters: vec!["9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin".to_owned()],
            account_filters: vec!["5KKsLVU6TcbVDK4BS6K1DGDxnh4Q9xjYJ8XaDCG5t8ht".to_owned()],
            ..Default::default()
        };

        let filter = Filter::new(&config);
        assert_eq!(filter.program_filters.len(), 1);
        assert_eq!(filter.account_filters.len(), 1);

        println!("{:?}", filter.account_filters);
        println!(
            "{:?}",
            &Pubkey::from_str("5KKsLVU6TcbVDK4BS6K1DGDxnh4Q9xjYJ8XaDCG5t8ht")
                .unwrap()
                .to_bytes()
        );

        assert!(filter.wants_program(
            &Pubkey::from_str("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin")
                .unwrap()
                .to_bytes()
        ));

        assert!(filter.wants_account(
            &Pubkey::from_str("5KKsLVU6TcbVDK4BS6K1DGDxnh4Q9xjYJ8XaDCG5t8ht")
                .unwrap()
                .to_bytes()
        ));
    }
}
