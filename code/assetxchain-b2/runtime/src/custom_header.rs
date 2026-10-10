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

//! Generic implementation of a block header.

use sp_runtime::{
	codec::{Codec, Decode, DecodeWithMemTracking, Encode},
	generic::Digest,
	scale_info::TypeInfo,
	traits::{self, AtLeast32BitUnsigned, BlockNumber, Hash as HashT, MaybeDisplay, Member},
};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use sp_core::{ U256, H256 };

/// Abstraction over a block header for a substrate chain.
#[derive(
	Encode, Decode, DecodeWithMemTracking, PartialEq, Eq, Clone, sp_core::RuntimeDebug, TypeInfo,
)]
#[scale_info(skip_type_params(Hash))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct CustomHeader<Number: Copy + Into<U256> + TryFrom<U256>, Hash: HashT> {
	/// The parent hash.
	pub parent_hash: Hash::Output,
	/// The block number.
	#[cfg_attr(
		feature = "serde",
		serde(serialize_with = "serialize_number", deserialize_with = "deserialize_number")
	)]
	#[codec(compact)]
	pub number: Number,
	/// The state trie merkle root
	pub state_root: Hash::Output,
	/// The merkle root of the extrinsics.
	pub extrinsics_root: Hash::Output,
	/// A chain-specific digest of data useful for light clients or referencing auxiliary data.
	pub digest: Digest,

	pub asset_root: H256,
}

#[cfg(feature = "serde")]
pub fn serialize_number<S, T: Copy + Into<U256> + TryFrom<U256>>(
	val: &T,
	s: S,
) -> Result<S::Ok, S::Error>
where
	S: serde::Serializer,
{
	let u256: U256 = (*val).into();
	serde::Serialize::serialize(&u256, s)
}

#[cfg(feature = "serde")]
pub fn deserialize_number<'a, D, T: Copy + Into<U256> + TryFrom<U256>>(d: D) -> Result<T, D::Error>
where
	D: serde::Deserializer<'a>,
{
	let u256: U256 = serde::Deserialize::deserialize(d)?;
	TryFrom::try_from(u256).map_err(|_| serde::de::Error::custom("Try from failed"))
}

impl<Number, Hash> traits::Header for CustomHeader<Number, Hash>
where
	Number: BlockNumber,
	Hash: HashT,
{
	type Number = Number;
	type Hash = <Hash as HashT>::Output;
	type Hashing = Hash;

	fn new(
		number: Self::Number,
		extrinsics_root: Self::Hash,
		state_root: Self::Hash,
		parent_hash: Self::Hash,
		digest: Digest,
	) -> Self {
		Self { number, extrinsics_root, state_root, parent_hash, digest, asset_root: Default::default(), }
	}
	fn number(&self) -> &Self::Number {
		&self.number
	}

	fn set_number(&mut self, num: Self::Number) {
		self.number = num
	}
	fn extrinsics_root(&self) -> &Self::Hash {
		&self.extrinsics_root
	}

	fn set_extrinsics_root(&mut self, root: Self::Hash) {
		self.extrinsics_root = root
	}
	fn state_root(&self) -> &Self::Hash {
		&self.state_root
	}

	fn set_state_root(&mut self, root: Self::Hash) {
		self.state_root = root
	}
	fn parent_hash(&self) -> &Self::Hash {
		&self.parent_hash
	}

	fn set_parent_hash(&mut self, hash: Self::Hash) {
		self.parent_hash = hash
	}

	fn digest(&self) -> &Digest {
		&self.digest
	}

	fn digest_mut(&mut self) -> &mut Digest {
		#[cfg(feature = "std")]
		log::debug!(target: "header", "Retrieving mutable reference to digest");
		&mut self.digest
	}
}

impl<Number, Hash> CustomHeader<Number, Hash>
where
	Number: Member
		+ core::hash::Hash
		+ Copy
		+ MaybeDisplay
		+ AtLeast32BitUnsigned
		+ Codec
		+ Into<U256>
		+ TryFrom<U256>,
	Hash: HashT,
{
	pub fn asset_root(&self) -> &H256 {
		&self.asset_root
	}

	/// 设置asset_root的值
	pub fn set_asset_root(&mut self, root: H256) {
		self.asset_root = root;
	}
	
	pub fn hash(&self) -> Hash::Output {
		Hash::hash_of(self)
	}
}

impl<Number, Hash> Default for CustomHeader<Number, Hash>
where
    Number: BlockNumber,
    Hash: HashT,
{
    fn default() -> Self {
        Self {
            parent_hash: Default::default(),
            number: Default::default(),
            state_root: Default::default(),
            extrinsics_root: Default::default(),
            digest: Default::default(),
            asset_root: H256::zero(),
        }
    }
}

pub trait AssetsStateRootProvider<Hash: sp_runtime::traits::Hash> {
    /// 计算当前资产状态根
    fn compute_assets_state_root() -> Hash::Output;
}