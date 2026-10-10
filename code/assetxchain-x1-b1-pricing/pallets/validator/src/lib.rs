// 网络验证节点+-
// 之后使用pallet-staking
// BABE\ValidatorSet Pallet\Root/Governance控制
// ValidatorSet+投票
// pallet-staking\NPoS\完整经济系统\与Polkadot同构

#![cfg_attr(not(feature = "std"), no_std)]
pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;
    use frame_support::traits::{
        Currency, ReservableCurrency, BuildGenesisConfig, ValidatorSet, ValidatorSetWithIdentification
    };
    use sp_runtime::traits::Convert;
    use sp_std::prelude::*;
    use sp_staking::offence::{Offence, ReportOffence, OffenceDetails, OnOffenceHandler, OffenceError};
    use pallet_im_online::UnresponsivenessOffence;

    pub type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub validators: BoundedVec<T::AccountId, T::MaxValidators>,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self { 
                validators: BoundedVec::default(),
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            Validators::<T>::put(&self.validators);
        }
    }

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_session::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        /// 使用 Balances 模块进行质押
        type Currency: ReservableCurrency<Self::AccountId>;
        /// 治理权限（Sudo 或 Council）
        type AddRemoveOrigin: EnsureOrigin<Self::RuntimeOrigin>;
        /// 最小质押金额
        #[pallet::constant]
        type MinValidatorBond: Get<BalanceOf<Self>>;
        // 验证节点数量上限
        #[pallet::constant]
        type MaxValidators: Get<u32>;
        /// 用于 ValidatorSet 的 Convert trait 实现
        type ValidatorIdOf: Convert<Self::AccountId, Option<Self::AccountId>>;
        /// 用于 ValidatorSetWithIdentification 的 Convert trait 实现
        type IdentificationOf: Convert<Self::AccountId, Option<Self::AccountId>>;
    }

    #[pallet::storage]
    #[pallet::getter(fn validators)]
    /// 存储当前的验证节点对应的账户的名单
    pub(super) type Validators<T: Config> = StorageValue<_, BoundedVec<T::AccountId, T::MaxValidators>, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        ValidatorAdded(T::AccountId),
        ValidatorRemoved(T::AccountId),
        ValidatorSlashed(T::AccountId, BalanceOf<T>),
    }

    #[pallet::error]
    pub enum Error<T> {
        NoAvailableKeys,
        AlreadyValidator,
        NotValidator,
        InsufficientBond,
        TooManyValidators,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// 添加验证人（治理调用）
        #[pallet::call_index(0)]
        #[pallet::weight({0})]
        pub fn add_validator(origin: OriginFor<T>, who: T::AccountId) -> DispatchResult {
            T::AddRemoveOrigin::ensure_origin(origin)?;
            
            // 处理上限
            Validators::<T>::try_mutate(|validators| {
                ensure!(!validators.contains(&who), Error::<T>::AlreadyValidator);

                // 尝试锁定质押
                T::Currency::reserve(&who, T::MinValidatorBond::get())?;

                // 尝试推入新成员，如果超过 MaxValidators 会返回错误
                validators.try_push(who.clone()).map_err(|_| Error::<T>::TooManyValidators)?;

                Ok::<(), DispatchError>(())
            })?;

            Self::deposit_event(Event::ValidatorAdded(who));
            Ok(())
        }

        /// 移除验证人并解锁资金（治理调用）
        #[pallet::call_index(1)]
        #[pallet::weight({0})]
        pub fn remove_validator(origin: OriginFor<T>, who: T::AccountId) -> DispatchResult {
            T::AddRemoveOrigin::ensure_origin(origin)?;

            Validators::<T>::try_mutate(|validators| {
                let pos = validators.iter().position(|x| x == &who)
                    .ok_or(Error::<T>::NotValidator)?;
                
                validators.remove(pos);
                
                // 解锁质押
                T::Currency::unreserve(&who, T::MinValidatorBond::get());
                
                Ok::<(), DispatchError>(())
            })?;

            Self::deposit_event(Event::ValidatorRemoved(who));
            Ok(())
        }

        // 验证节点质押调整
    }

    // 对接Session模块
    impl<T: Config> pallet_session::SessionManager<T::AccountId> for Pallet<T> {
        fn new_session(_index: u32) -> Option<Vec<T::AccountId>> {
            let validators = Self::validators();
            if validators.is_empty() {
                None
            } else {
                // 将BoundedVec换成Session要求的Vec
                Some(validators.to_vec())
            }
        }
        fn start_session(_index: u32) {}
        fn end_session(_index: u32) {}
    }

    pub struct ImOnlineOffence<T: Config> {
        offender: T::AccountId,
        session_index: u32,
    }

	impl<T: Config> Offence<T::AccountId> for ImOnlineOffence<T> {
        const ID: [u8; 16] = *b"im-online:offenc";
        type TimeSlot = u32;

        fn offenders(&self) -> Vec<T::AccountId> {
            vec![self.offender.clone()]
        }

        fn session_index(&self) -> u32 {
            self.session_index
        }

        fn validator_set_count(&self) -> u32 {
            // 从存储读取当前验证人总数
            Pallet::<T>::validators().len() as u32
        }

        fn time_slot(&self) -> Self::TimeSlot {
            self.session_index
        }

        fn slash_fraction(&self, _offenders_count: u32) -> sp_runtime::Perbill {
            sp_runtime::Perbill::from_percent(10)
        }
    }

    impl<T: Config> ReportOffence<
        T::AccountId,
        (T::AccountId, T::AccountId),
        UnresponsivenessOffence<(T::AccountId, T::AccountId)>
    > for Pallet<T> {
        fn report_offence(
            _reporters: Vec<T::AccountId>,
            _offence: UnresponsivenessOffence<(T::AccountId, T::AccountId)>,
        ) -> Result<(), OffenceError> {
            // im-online发现违规后调用
            Ok(())
        }

        fn is_known_offence(_offenders: &[(T::AccountId, T::AccountId)], _time_slot: &u32 ) -> bool {
            false
        }
    }

    impl<T: Config> OnOffenceHandler<(T::AccountId, T::AccountId), (T::AccountId, T::AccountId), DispatchError> for Pallet<T> {
		fn on_offence(
            offenders: &[OffenceDetails<(T::AccountId, T::AccountId), (T::AccountId, T::AccountId)>],
            _slash_fraction: &[sp_runtime::Perbill],
            _slash_session: u32,
        ) -> DispatchError {
            for detail in offenders {
                let (offender_acc, _identification) = &detail.offender; // 获取元组中的 AccountId
                let slash_amount = T::MinValidatorBond::get();
                // 这里是全部罚款，应该为按比例罚款
                let (imbalance, _) = T::Currency::slash_reserved(offender_acc, slash_amount);
                drop(imbalance);

                Validators::<T>::mutate(|v| {
                    if let Some(pos) = v.iter().position(|x| x == offender_acc) {
                        v.remove(pos);
                    }
                });
                Self::deposit_event(Event::ValidatorSlashed(offender_acc.clone(), slash_amount));
            }
            DispatchError::Other("Success")
        }
	}

    impl<T: Config> ValidatorSet<T::AccountId> for Pallet<T> {
        type ValidatorId = T::AccountId;
        type ValidatorIdOf = <T as pallet::Config>::ValidatorIdOf;

        fn validators() -> Vec<Self::ValidatorId> {
            Validators::<T>::get().to_vec()
        }

        fn session_index() -> u32 {
            pallet_session::Pallet::<T>::current_index()
        }
    }

    impl<T: Config> ValidatorSetWithIdentification<T::AccountId> for Pallet<T> {
        type Identification = T::AccountId;
        type IdentificationOf = T::IdentificationOf; 
    }
}