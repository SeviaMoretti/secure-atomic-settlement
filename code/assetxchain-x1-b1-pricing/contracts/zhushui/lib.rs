#![cfg_attr(not(feature = "std"), no_std, no_main)]

#[ink::contract]
pub mod traditional_asset {
    use ink::prelude::string::String;
    use ink::storage::Mapping;

    // 1. 定义错误类型
    #[derive(scale::Encode, scale::Decode, Debug, PartialEq, Eq)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum Error {
        AssetAlreadyExists,
        AssetNotFound,
        NotOwner,
    }

    // 2. 模拟 DataAsset 结构
    #[derive(scale::Encode, scale::Decode, Debug, PartialEq, Eq)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct Asset {
        name: String,
        description: String,
        raw_data_hash: [u8; 32], 
        data_size_bytes: u64,
    }

    // 3. 全局状态表：引发“状态爆炸”的单层 MPT 对照组核心
    #[ink(storage)]
    pub struct TraditionalAsset {
        owners: Mapping<u64, AccountId>,
        assets: Mapping<u64, Asset>,
    }

    // 4. 定义事件
    #[ink(event)]
    pub struct AssetRegistered {
        #[ink(topic)]
        asset_id: u64,
        #[ink(topic)]
        owner: AccountId,
    }

    #[ink(event)]
    pub struct AssetTransferred {
        #[ink(topic)]
        asset_id: u64,
        #[ink(topic)]
        from: AccountId,
        #[ink(topic)]
        to: AccountId,
    }

    impl TraditionalAsset {
        // 构造函数
        #[ink(constructor)]
        pub fn new() -> Self {
            Self {
                owners: Mapping::default(),
                assets: Mapping::default(),
            }
        }

        // 1. 注册资产操作
        #[ink(message)]
        pub fn register_asset(
            &mut self,
            asset_id: u64,
            name: String,
            description: String,
            raw_data_hash: [u8; 32],
            data_size_bytes: u64,
        ) -> Result<(), Error> {
            let caller = self.env().caller();

            // 检查资产是否已存在
            if self.owners.contains(asset_id) {
                return Err(Error::AssetAlreadyExists);
            }

            self.owners.insert(asset_id, &caller);

            let asset = Asset {
                name,
                description,
                raw_data_hash,
                data_size_bytes,
            };
            self.assets.insert(asset_id, &asset);

            self.env().emit_event(AssetRegistered {
                asset_id,
                owner: caller,
            });

            Ok(())
        }

        // 2. 转移资产操作
        #[ink(message)]
        pub fn transfer_asset(
            &mut self,
            asset_id: u64,
            new_owner: AccountId,
        ) -> Result<(), Error> {
            let caller = self.env().caller();

            let current_owner = self.owners.get(asset_id).ok_or(Error::AssetNotFound)?;
            if current_owner != caller {
                return Err(Error::NotOwner);
            }

            self.owners.insert(asset_id, &new_owner);

            self.env().emit_event(AssetTransferred {
                asset_id,
                from: caller,
                to: new_owner,
            });

            Ok(())
        }
    }
}