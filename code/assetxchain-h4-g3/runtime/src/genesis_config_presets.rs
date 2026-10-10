// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{AccountId, BalancesConfig, FOUNDATION_PERCENT, INCENTIVE_POOL_PERCENT, MINING_REWARD_PERCENT, RuntimeGenesisConfig, SessionKeys, SudoConfig, UNIT
};
use crate::configs::FoundationVestingPeriod;
use alloc::{vec, vec::Vec};
use frame_support::build_struct_json_patch;
use serde_json::Value;
use sp_consensus_babe::AuthorityId as BabeId;
use sp_consensus_grandpa::AuthorityId as GrandpaId;
use sp_genesis_builder::{self, PresetId};
use sp_keyring::Sr25519Keyring;
use sp_core::crypto::UncheckedFrom;
use sp_runtime::AccountId32;
use hex_literal::hex;

fn session_keys(babe: BabeId, grandpa: GrandpaId) -> SessionKeys {
    SessionKeys { babe, grandpa }
}

// 预设账户，公钥应该使用线下生成，确保安全
fn foundation_account() -> AccountId {
    // 基金会账户 (可以使用固定公钥)
    AccountId32::unchecked_from(sp_core::H256(hex!("701f9c89ca77bc72933cd3c1d29b18cc9fc3a95bb4b9f137373e9ebf9239b34f"))).into()
}

fn incentive_pool_account() -> AccountId {
    // 激励池账户
    AccountId32::unchecked_from(sp_core::H256(hex!("1a9de66d5ca5a6a7bad9add630d85b972f351082b0422e5f64c78a4eecc4a427"))).into()
}

// Returns the genesis config presets populated with given parameters.
fn testnet_genesis(
	initial_authorities: Vec<(AccountId, BabeId, GrandpaId)>,
	endowed_accounts: Vec<AccountId>,
	root: AccountId,
) -> Value {
	// 合并初始账户和预分配账户
    let mut all_endowed = endowed_accounts.clone();
    let foundation = foundation_account();
    let incentive_pool = incentive_pool_account();
    
    if !all_endowed.contains(&foundation) {
        all_endowed.push(foundation.clone());
    }
    if !all_endowed.contains(&incentive_pool) {
        all_endowed.push(incentive_pool.clone());
    }

	build_struct_json_patch!(RuntimeGenesisConfig {
		balances: BalancesConfig {
			balances: all_endowed
				.iter()
				.cloned()
				.map(|k| {
					if k == foundation {
						(k, FOUNDATION_PERCENT)
					} else if k == incentive_pool {
						(k, INCENTIVE_POOL_PERCENT)
					} else {
						(k, 1<<100)
					}
				})
				.collect::<Vec<_>>(),
		},
		babe: pallet_babe::GenesisConfig {
            authorities: vec![],
			// authorities: initial_authorities
            //     .iter()
            //     .map(|x| (x.1.clone(), 1))
            //     .collect::<Vec<_>>(),
            epoch_config: sp_consensus_babe::BabeEpochConfiguration {
                c: (1, 4), // 25% 的slots预期会有区块
                allowed_slots: sp_consensus_babe::AllowedSlots::PrimaryAndSecondaryVRFSlots,
            },
        },
		grandpa: pallet_grandpa::GenesisConfig {
			authorities: vec![],
			// authorities: initial_authorities.iter().map(|x| (x.2.clone(), 1)).collect::<Vec<_>>(),
		},
		session: pallet_session::GenesisConfig {
            keys: initial_authorities
                .iter()
                .map(|x| {
                    (
                        x.0.clone(), // account id
                        x.0.clone(), // validator id
                        session_keys(x.1.clone(), x.2.clone()),
                    )
                })
                .collect::<Vec<_>>(),
        },
		sudo: SudoConfig { key: Some(root) },
		vesting: pallet_vesting::GenesisConfig {
			vesting: vec![
				(
					foundation_account(),
					0,                                    // 从区块0开始
					FoundationVestingPeriod::get() / 5,          // 持续1年
					FOUNDATION_PERCENT * 20 / 100,        // 第一年释放20%
				),
				// 第二年释放 20%  
				(
					foundation_account(),
					FoundationVestingPeriod::get() / 5,          // 从第1年开始
					FoundationVestingPeriod::get() / 5,          // 持续1年
					FOUNDATION_PERCENT * 20 / 100,        // 第二年释放20%
				),
				// 第三年释放 20%
				(
					foundation_account(),
					FoundationVestingPeriod::get() * 2 / 5,      // 从第2年开始
					FoundationVestingPeriod::get() / 5,          // 持续1年
					FOUNDATION_PERCENT * 20 / 100,        // 第三年释放20%
				),
				// 第四年释放 20%
				(
					foundation_account(),
					FoundationVestingPeriod::get() * 3 / 5,      // 从第3年开始
					FoundationVestingPeriod::get() / 5,          // 持续1年
					FOUNDATION_PERCENT * 20 / 100,        // 第四年释放20%
				),
				// 第五年释放 20%
				(
					foundation_account(),
					FoundationVestingPeriod::get() * 4 / 5,      // 从第4年开始
					FoundationVestingPeriod::get() / 5,          // 持续1年
					FOUNDATION_PERCENT * 20 / 100,        // 第五年释放20%
				),
			],
		},
		validator: pallet_validator::GenesisConfig {
			validators: initial_authorities
				.iter()
				.map(|x| x.0.clone()) // 提取初始验证者的账户ID
				.collect::<Vec<_>>()
				.try_into() // 转换为BoundedVec（处理长度超限错误）
				.expect("初始验证者数量不超过MaxValidators"),
		},
		incentive: pallet_incentive::GenesisConfig {
            _marker: Default::default(),
        },
	})
}

/// Return the development genesis config.
pub fn development_config_genesis() -> Value {
	testnet_genesis(
		vec![(
			sp_keyring::Sr25519Keyring::Alice.to_account_id(),
			sp_keyring::Sr25519Keyring::Alice.public().into(),
			sp_keyring::Ed25519Keyring::Alice.public().into(),
		)],
		vec![
			Sr25519Keyring::Alice.to_account_id(),
			Sr25519Keyring::Bob.to_account_id(),
			Sr25519Keyring::AliceStash.to_account_id(),
			Sr25519Keyring::BobStash.to_account_id(),
		],
		sp_keyring::Sr25519Keyring::Alice.to_account_id(),
	)
}

/// Return the local genesis config preset.
pub fn local_config_genesis() -> Value {
	testnet_genesis(
		vec![
			(
				sp_keyring::Sr25519Keyring::Alice.to_account_id(),
				sp_keyring::Sr25519Keyring::Alice.public().into(),
				sp_keyring::Ed25519Keyring::Alice.public().into(),
			),
			(
				sp_keyring::Sr25519Keyring::Bob.to_account_id(),
				sp_keyring::Sr25519Keyring::Bob.public().into(),
				sp_keyring::Ed25519Keyring::Bob.public().into(),
			),
		],
		Sr25519Keyring::iter()
			.filter(|v| v != &Sr25519Keyring::One && v != &Sr25519Keyring::Two)
			.map(|v| v.to_account_id())
			.collect::<Vec<_>>(),
		Sr25519Keyring::Alice.to_account_id(),
	)
}

/// Provides the JSON representation of predefined genesis config for given `id`.
pub fn get_preset(id: &PresetId) -> Option<Vec<u8>> {
	let patch = match id.as_ref() {
		sp_genesis_builder::DEV_RUNTIME_PRESET => development_config_genesis(),
		sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET => local_config_genesis(),
		_ => return None,
	};
	Some(
		serde_json::to_string(&patch)
			.expect("serialization to json is expected to work. qed.")
			.into_bytes(),
	)
}

/// List of supported presets.
pub fn preset_names() -> Vec<PresetId> {
	vec![
		PresetId::from(sp_genesis_builder::DEV_RUNTIME_PRESET),
		PresetId::from(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET),
	]
}
