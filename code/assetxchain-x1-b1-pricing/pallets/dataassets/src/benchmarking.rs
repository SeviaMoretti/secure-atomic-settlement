//! Benchmarking setup for pallet-dataassets

use super::*;
use crate::Pallet as DataAssets;
use frame_benchmarking::v2::*;
use frame_support::traits::{ Currency, Get };
use frame_system::RawOrigin;
use sp_core::H256;
use sp_runtime::traits::{ Saturating, SaturatedConversion };
use sp_std::vec;

// 为基准测试创建账户并提供资金
fn create_funded_account<T: Config>(name: &'static str, index: u32) -> T::AccountId {
    let account: T::AccountId = frame_benchmarking::account(name, index, 0);
    let balance = T::Currency::minimum_balance() * 1000u32.into();
    T::Currency::make_free_balance_be(&account, balance);
    account
}

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn register_asset() {
        // 参数
        let caller = create_funded_account::<T>("caller", 0);
        let name = vec![b'T'; T::MaxNameLength::get() as usize];
        let description = vec![b'D'; T::MaxDescriptionLength::get() as usize];
        let raw_data_hash = H256::repeat_byte(0x01);
        let data_size_bytes = 1024 * 1024; // 1 MB

        // 确保有足够的质押金
        let collateral = T::BaseCollateral::get()
            .saturating_add(T::CollateralPerMB::get());
        T::Currency::make_free_balance_be(&caller, collateral * 10u32.into());

        #[extrinsic_call]
        register_asset(
            RawOrigin::Signed(caller.clone()),
            name,
            description,
            raw_data_hash,
            data_size_bytes,
        );

        // 验证
        assert!(DataAssets::<T>::get_asset(&[0u8; 32]).is_some() 
            || frame_system::Pallet::<T>::events().len() > 0);
    }

    #[benchmark]
    fn register_asset_core() {
        let caller = create_funded_account::<T>("caller", 0);
        let name = vec![b'T'; T::MaxNameLength::get() as usize];
        let description = vec![b'D'; T::MaxDescriptionLength::get() as usize];
        let raw_data_hash = H256::repeat_byte(0x02);
        let data_size_bytes = 1024 * 1024;

        #[extrinsic_call]
        register_asset_core(
            RawOrigin::Signed(caller.clone()),
            name,
            description,
            raw_data_hash,
            data_size_bytes,
        );

        assert!(frame_system::Pallet::<T>::events().len() > 0);
    }

    #[benchmark]
    fn issue_certificate() {
        // 先注册资产
        let owner = create_funded_account::<T>("owner", 0);
        let holder = create_funded_account::<T>("holder", 1);
        
        let name = b"Test Asset".to_vec();
        let description = b"Test Description".to_vec();
        let raw_data_hash = H256::repeat_byte(0x01);
        let data_size_bytes = 1024 * 1024;

        let collateral = T::BaseCollateral::get()
            .saturating_add(T::CollateralPerMB::get());
        T::Currency::make_free_balance_be(&owner, collateral * 10u32.into());

        // 注册资产
        assert!(DataAssets::<T>::register_asset(
            RawOrigin::Signed(owner.clone()).into(),
            name.clone(),
            description.clone(),
            raw_data_hash,
            data_size_bytes,
        ).is_ok());

        // 获取生成的 asset_id
        let timestamp = <pallet_timestamp::Pallet<T>>::get().saturated_into::<u64>();
        let asset_id = crate::types::DataAsset::generate_asset_id(&owner, timestamp, &raw_data_hash);

        #[extrinsic_call]
        issue_certificate(
            RawOrigin::Signed(owner.clone()),
            asset_id,
            holder,
            1u8, // Usage right
            None, // No expiration
        );

        // 验证
        assert!(frame_system::Pallet::<T>::events().len() > 0);
    }

    #[benchmark]
    fn transfer_asset() {
        // 设置
        let owner = create_funded_account::<T>("owner", 0);
        let new_owner = create_funded_account::<T>("new_owner", 1);
        
        let name = b"Test Asset".to_vec();
        let description = b"Test Description".to_vec();
        let raw_data_hash = H256::repeat_byte(0x01);
        let data_size_bytes = 1024 * 1024;

        let collateral = T::BaseCollateral::get()
            .saturating_add(T::CollateralPerMB::get());
        T::Currency::make_free_balance_be(&owner, collateral * 10u32.into());

        assert!(DataAssets::<T>::register_asset(
            RawOrigin::Signed(owner.clone()).into(),
            name,
            description,
            raw_data_hash,
            data_size_bytes,
        ).is_ok());

        let timestamp = <pallet_timestamp::Pallet<T>>::get().saturated_into::<u64>();
        let asset_id = crate::types::DataAsset::generate_asset_id(&owner, timestamp, &raw_data_hash);

        #[extrinsic_call]
        transfer_asset(
            RawOrigin::Signed(owner.clone()),
            asset_id,
            new_owner,
        );

        // 验证
        assert!(frame_system::Pallet::<T>::events().len() > 0);
    }

    #[benchmark]
    fn revoke_certificate() {
        // 设置
        let owner = create_funded_account::<T>("owner", 0);
        let holder = create_funded_account::<T>("holder", 1);
        
        let name = b"Test Asset".to_vec();
        let description = b"Test Description".to_vec();
        let raw_data_hash = H256::repeat_byte(0x01);
        let data_size_bytes = 1024 * 1024;

        let collateral = T::BaseCollateral::get()
            .saturating_add(T::CollateralPerMB::get());
        T::Currency::make_free_balance_be(&owner, collateral * 10u32.into());

        assert!(DataAssets::<T>::register_asset(
            RawOrigin::Signed(owner.clone()).into(),
            name,
            description,
            raw_data_hash,
            data_size_bytes,
        ).is_ok());

        // 关键：在 issue_certificate 之前获取时间戳，确保一致性
        let timestamp = <pallet_timestamp::Pallet<T>>::get().saturated_into::<u64>();
        let asset_id = crate::types::DataAsset::generate_asset_id(&owner, timestamp, &raw_data_hash);

        // 在同一时间点发证
        assert!(DataAssets::<T>::issue_certificate(
            RawOrigin::Signed(owner.clone()).into(),
            asset_id,
            holder.clone(),
            1u8,
            None,
        ).is_ok());

        // 生成正确的 certificate_id
        // 根据 lib.rs 中 issue_certificate 的实现：
        // - 使用 current_time (当前时间戳)
        // - 使用 asset.owner 作为 issuer (不是 holder!)
        // - RightToken::minimal 内部调用 generate_certificate_id(&parent_asset_id, current_time, &issuer)
        let certificate_id = crate::types::RightToken::<T::AccountId>::generate_certificate_id(
            &asset_id,
            timestamp,  // 使用同一个时间戳
            &owner,     // 使用 owner 作为 issuer (不是 holder!)
            0u32,
        );

        #[extrinsic_call]
        revoke_certificate(
            RawOrigin::Signed(owner.clone()),
            asset_id,
            certificate_id,
        );
    }

    #[benchmark]
    fn lock_asset() {
        // 设置
        let owner = create_funded_account::<T>("owner", 0);
        
        let name = b"Test Asset".to_vec();
        let description = b"Test Description".to_vec();
        let raw_data_hash = H256::repeat_byte(0x01);
        let data_size_bytes = 1024 * 1024;

        let collateral = T::BaseCollateral::get()
            .saturating_add(T::CollateralPerMB::get());
        T::Currency::make_free_balance_be(&owner, collateral * 10u32.into());

        assert!(DataAssets::<T>::register_asset(
            RawOrigin::Signed(owner.clone()).into(),
            name,
            description,
            raw_data_hash,
            data_size_bytes,
        ).is_ok());

        let timestamp = <pallet_timestamp::Pallet<T>>::get().saturated_into::<u64>();
        let asset_id = crate::types::DataAsset::generate_asset_id(&owner, timestamp, &raw_data_hash);

        #[extrinsic_call]
        lock_asset(RawOrigin::Signed(owner.clone()), asset_id);
    }

    #[benchmark]
    fn unlock_asset() {
        // 设置
        let owner = create_funded_account::<T>("owner", 0);
        
        let name = b"Test Asset".to_vec();
        let description = b"Test Description".to_vec();
        let raw_data_hash = H256::repeat_byte(0x01);
        let data_size_bytes = 1024 * 1024;

        let collateral = T::BaseCollateral::get()
            .saturating_add(T::CollateralPerMB::get());
        T::Currency::make_free_balance_be(&owner, collateral * 10u32.into());

        assert!(DataAssets::<T>::register_asset(
            RawOrigin::Signed(owner.clone()).into(),
            name,
            description,
            raw_data_hash,
            data_size_bytes,
        ).is_ok());

        let timestamp = <pallet_timestamp::Pallet<T>>::get().saturated_into::<u64>();
        let asset_id = crate::types::DataAsset::generate_asset_id(&owner, timestamp, &raw_data_hash);

        assert!(DataAssets::<T>::lock_asset(
            RawOrigin::Signed(owner.clone()).into(),
            asset_id,
        ).is_ok());

        #[extrinsic_call]
        unlock_asset(RawOrigin::Signed(owner.clone()), asset_id);
    }

    #[benchmark]
    fn slash_collateral() {
        // 设置
        let owner = create_funded_account::<T>("owner", 0);
        
        let name = b"Test Asset".to_vec();
        let description = b"Test Description".to_vec();
        let raw_data_hash = H256::repeat_byte(0x01);
        let data_size_bytes = 1024 * 1024;

        let collateral = T::BaseCollateral::get()
            .saturating_add(T::CollateralPerMB::get());
        T::Currency::make_free_balance_be(&owner, collateral * 10u32.into());

        assert!(DataAssets::<T>::register_asset(
            RawOrigin::Signed(owner.clone()).into(),
            name,
            description,
            raw_data_hash,
            data_size_bytes,
        ).is_ok());

        let timestamp = <pallet_timestamp::Pallet<T>>::get().saturated_into::<u64>();
        let asset_id = crate::types::DataAsset::generate_asset_id(&owner, timestamp, &raw_data_hash);

        let slash_percentage = 50u8;

        #[extrinsic_call]
        slash_asset_collateral(RawOrigin::Root, asset_id, slash_percentage);
    }

    #[benchmark]
    fn authorize_operator() {
        // 设置
        let owner = create_funded_account::<T>("owner", 0);
        let market = create_funded_account::<T>("market", 1);
        
        let name = b"Test Asset".to_vec();
        let description = b"Test Description".to_vec();
        let raw_data_hash = H256::repeat_byte(0x01);
        let data_size_bytes = 1024 * 1024;

        let collateral = T::BaseCollateral::get()
            .saturating_add(T::CollateralPerMB::get());
        T::Currency::make_free_balance_be(&owner, collateral * 10u32.into());

        assert!(DataAssets::<T>::register_asset(
            RawOrigin::Signed(owner.clone()).into(),
            name,
            description,
            raw_data_hash,
            data_size_bytes,
        ).is_ok());

        let timestamp = <pallet_timestamp::Pallet<T>>::get().saturated_into::<u64>();
        let asset_id = crate::types::DataAsset::generate_asset_id(&owner, timestamp, &raw_data_hash);

        #[extrinsic_call]
        authorize_market(
            RawOrigin::Signed(owner.clone()),
            asset_id,
            market,
        );
    }

    #[benchmark]
    fn revoke_authorization() {
        // 设置
        let owner = create_funded_account::<T>("owner", 0);
        let market = create_funded_account::<T>("market", 1);
        
        let name = b"Test Asset".to_vec();
        let description = b"Test Description".to_vec();
        let raw_data_hash = H256::repeat_byte(0x01);
        let data_size_bytes = 1024 * 1024;

        let collateral = T::BaseCollateral::get()
            .saturating_add(T::CollateralPerMB::get());
        T::Currency::make_free_balance_be(&owner, collateral * 10u32.into());

        assert!(DataAssets::<T>::register_asset(
            RawOrigin::Signed(owner.clone()).into(),
            name,
            description,
            raw_data_hash,
            data_size_bytes,
        ).is_ok());

        let timestamp = <pallet_timestamp::Pallet<T>>::get().saturated_into::<u64>();
        let asset_id = crate::types::DataAsset::generate_asset_id(&owner, timestamp, &raw_data_hash);

        assert!(DataAssets::<T>::authorize_market(
            RawOrigin::Signed(owner.clone()).into(),
            asset_id,
            market,
        ).is_ok());

        #[extrinsic_call]
        revoke_authorization(RawOrigin::Signed(owner.clone()), asset_id);
    }

    #[benchmark]
    fn register_asset_under_load(
        // 从 1 万到 10 万，步长由 --steps 决定
        x: Linear<10_000, 100_000>, 
    ) {
        let caller = create_funded_account::<T>("caller", 0);
        let collateral = T::BaseCollateral::get().saturating_add(T::CollateralPerMB::get());
        T::Currency::make_free_balance_be(&caller, collateral * 100_000u32.into());

        // 1. 模拟向主资产树（Asset Trie Child-Trie）中疯狂注水 x 个资产
        // 我们直接调用 lib.rs 中你自己写的子树写入方法，完全越过上层业务逻辑，极速撑爆 RocksDB
        for i in 0..x {
            // 生成一个模拟的 hash 和 [u8; 32] 类型的 asset_id
            // let dummy_hash = H256::from_low_u64_be(i as u64);
            // let dummy_asset_id: [u8; 32] = dummy_hash.into();
            let mut hash_bytes = [0u8; 32];
            let i_bytes = (i as u64).to_be_bytes();
            hash_bytes[24..32].copy_from_slice(&i_bytes);

            let dummy_hash = H256::from(hash_bytes);
            let dummy_asset_id: [u8; 32] = hash_bytes;

            // 使用你在 types.rs 中规范的 minimal 构造器，防止结构体字段缺失报错
            let mut dummy_asset = crate::types::DataAsset::minimal(
                caller.clone(),
                b"Dummy".to_vec(),
                b"Dummy Desc".to_vec(),
                dummy_hash,       // raw_data_hash
                0u64,             // timestamp
            );
            
            // 补全特定字段
            dummy_asset.core.asset_id = dummy_asset_id;
            dummy_asset.core.token_id = i;

            // 调用 lib.rs 里的内部方法写入 Child Trie (注: benchmarking 和 lib 同属一个 crate，可直接访问此非 pub 方法)
            let _ = crate::Pallet::<T>::insert_asset(&dummy_asset_id, &dummy_asset);
        }

        let name = b"Target Asset".to_vec();
        let description = b"Target Desc".to_vec();
        let target_hash = H256::repeat_byte(0xFF); // 发起真实交易的目标资产哈希

        // 开始计时（Benchmark 框架只记录这块执行的时间）
        #[extrinsic_call]
        register_asset(
            RawOrigin::Signed(caller.clone()),
            name,
            description,
            target_hash,
            1024,
        );
        
        // 验证注册是否成功
        assert!(frame_system::Pallet::<T>::events().len() > 0);
    }

    impl_benchmark_test_suite!(DataAssets, crate::tests::new_test_ext(), crate::tests::Test);
}
