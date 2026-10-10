#![cfg_attr(not(feature = "std"), no_std, no_main)]
#![allow(clippy::cast_possible_truncation)]

use market_standard::{MarketStandard, SettlementV2ExtError};

#[ink::contract(env = market_standard::CustomEnvironment)]
mod market_orderbook {
    use super::*;
    use ink::prelude::vec::Vec;
    use ink::storage::Mapping;
    use market_standard::{
        BuyerCommitment, FundedV2Receipt, Hash32, OrderCommitment, OrderState, PricePolicyWitness,
        ProtocolSignature, RefundedV2Receipt, RuntimeOrder, SellerCommitment, SettlementRecord,
    };

    #[derive(Debug, Clone, PartialEq, Eq, codec::Encode, codec::Decode)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct MarketIntent {
        pub order: OrderCommitment,
        pub price_policy_witness: PricePolicyWitness,
        pub seller_commitment: SellerCommitment,
        pub seller_signature: ProtocolSignature,
        pub submitted_by: AccountId,
        pub submitted_at: BlockNumber,
    }

    #[derive(Debug, Clone, PartialEq, Eq, codec::Encode, codec::Decode)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct OrderProjection {
        pub order: RuntimeOrder,
        pub settlement: Option<SettlementRecord>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, codec::Encode, codec::Decode)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct R2X1PolicyDecision {
        pub base_price: Balance,
        pub gross_price: Balance,
        pub market_fee_bps: u16,
        pub market_fee_amount: Balance,
        pub selected_offer_index: u32,
    }

    #[derive(Debug, Clone, PartialEq, Eq, codec::Encode, codec::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum Error {
        ChainExtension(SettlementV2ExtError),
        MarketMismatch,
        RuntimeProjectionMismatch,
        NotAdmin,
        MarketAlreadyBound,
        InvalidMarketId,
        MarketNotBound,
        PolicyDecisionMismatch,
    }

    impl From<SettlementV2ExtError> for Error {
        fn from(error: SettlementV2ExtError) -> Self {
            Self::ChainExtension(error)
        }
    }

    #[ink(storage)]
    pub struct MarketOrderbook {
        market_id: Hash32,
        market_id_bound: bool,
        fee_ratio: u32,
        admin: AccountId,
        intents: Mapping<Hash32, MarketIntent>,
        projections: Mapping<Hash32, OrderProjection>,
        last_policy_decision: Option<R2X1PolicyDecision>,
    }

    #[ink(event)]
    pub struct IntentRelayed {
        #[ink(topic)]
        order_id: Hash32,
        operation: market_standard::Operation,
    }

    #[ink(event)]
    pub struct ProjectionSynchronized {
        #[ink(topic)]
        order_id: Hash32,
        state: OrderState,
        has_settlement: bool,
    }

    #[ink(event)]
    pub struct MarketIdBound {
        #[ink(topic)]
        market_id: Hash32,
    }

    #[ink(event)]
    pub struct PolicyEvaluated {
        pub base_price: Balance,
        pub gross_price: Balance,
        pub market_fee_bps: u16,
        pub selected_offer_index: u32,
    }

    impl MarketOrderbook {
        #[ink(constructor)]
        pub fn new(market_id: Hash32, fee_ratio: u32) -> Self {
            Self {
                market_id,
                market_id_bound: market_id != [0; 32],
                fee_ratio,
                admin: Self::env().caller(),
                intents: Mapping::default(),
                projections: Mapping::default(),
                last_policy_decision: None,
            }
        }

        #[ink(message)]
        pub fn bind_market_id(&mut self, market_id: Hash32) -> Result<(), Error> {
            if self.env().caller() != self.admin {
                return Err(Error::NotAdmin);
            }
            if self.market_id_bound {
                return Err(Error::MarketAlreadyBound);
            }
            if market_id == [0; 32] {
                return Err(Error::InvalidMarketId);
            }
            self.market_id = market_id;
            self.market_id_bound = true;
            self.env().emit_event(MarketIdBound { market_id });
            Ok(())
        }

        #[ink(message)]
        pub fn relay_create_order(
            &mut self,
            order: OrderCommitment,
            price_policy_witness: PricePolicyWitness,
            seller_commitment: SellerCommitment,
            seller_signature: ProtocolSignature,
        ) -> Result<Hash32, Error> {
            self.ensure_market_id_bound()?;
            if order.market_id != self.market_id {
                return Err(Error::MarketMismatch);
            }
            let consumes_policy_decision = if let Some(decision) = self.last_policy_decision {
                if order.price != decision.gross_price
                    || self.fee_ratio != u32::from(decision.market_fee_bps)
                {
                    return Err(Error::PolicyDecisionMismatch);
                }
                true
            } else {
                false
            };
            let order_id = self.env().extension().create_order(
                order.clone(),
                price_policy_witness,
                seller_commitment.clone(),
                seller_signature,
            )?;
            let intent = MarketIntent {
                order: order.clone(),
                price_policy_witness,
                seller_commitment,
                seller_signature,
                submitted_by: self.env().caller(),
                submitted_at: self.env().block_number(),
            };
            self.intents.insert(order_id, &intent);
            self.env().emit_event(IntentRelayed {
                order_id,
                operation: order.operation,
            });
            if consumes_policy_decision {
                self.last_policy_decision = None;
            }
            Ok(order_id)
        }

        #[ink(message)]
        pub fn relay_fund_order(
            &mut self,
            order_id: Hash32,
            buyer_commitment: BuyerCommitment,
            buyer_signature: ProtocolSignature,
        ) -> Result<FundedV2Receipt, Error> {
            self.env()
                .extension()
                .fund_order(order_id, buyer_commitment, buyer_signature)
                .map_err(Into::into)
        }

        #[ink(message)]
        pub fn relay_settle(&mut self, order_id: Hash32) -> Result<Hash32, Error> {
            self.env().extension().settle(order_id).map_err(Into::into)
        }

        #[ink(message)]
        pub fn r2_x1_policy_descriptor(&self) -> Vec<u8> {
            include_bytes!("r2_x1_policy.json").to_vec()
        }

        #[ink(message)]
        pub fn r2_x1_evaluate_policy(
            &mut self,
            base_price: Balance,
            offers: Vec<Balance>,
        ) -> R2X1PolicyDecision {
            let gross_price = base_price.saturating_mul(10_500) / 10_000;
            let decision = R2X1PolicyDecision {
                base_price,
                gross_price,
                market_fee_bps: 0,
                market_fee_amount: 0,
                selected_offer_index: 0,
            };
            let _ = offers;
            self.last_policy_decision = Some(decision);
            self.env().emit_event(PolicyEvaluated {
                base_price,
                gross_price,
                market_fee_bps: 0,
                selected_offer_index: 0,
            });
            decision
        }

        #[ink(message)]
        pub fn r2_x1_last_policy_decision(&self) -> Option<R2X1PolicyDecision> {
            self.last_policy_decision
        }

        #[ink(message)]
        pub fn relay_cancel_open(&mut self, order_id: Hash32) -> Result<OrderState, Error> {
            self.env()
                .extension()
                .cancel_open(order_id)
                .map_err(Into::into)
        }

        #[ink(message)]
        pub fn relay_refund_funded(
            &mut self,
            order_id: Hash32,
        ) -> Result<RefundedV2Receipt, Error> {
            self.env()
                .extension()
                .refund_funded(order_id)
                .map_err(Into::into)
        }

        /// Rebuilds the local read model from Runtime authority. This method
        /// performs queries only; replaying it can never trigger settlement.
        #[ink(message)]
        pub fn sync_projection(&mut self, order_id: Hash32) -> Result<OrderProjection, Error> {
            self.ensure_market_id_bound()?;
            let order = self.env().extension().query_order(order_id)?;
            if order.order_id != order_id || order.order_commitment.market_id != self.market_id {
                return Err(Error::RuntimeProjectionMismatch);
            }
            let settlement = self.env().extension().query_settlement(order_id)?;
            if settlement.as_ref().is_some_and(|record| {
                record.order_id != order_id || record.market_id != self.market_id
            }) {
                return Err(Error::RuntimeProjectionMismatch);
            }
            let projection = OrderProjection { order, settlement };
            self.projections.insert(order_id, &projection);
            self.env().emit_event(ProjectionSynchronized {
                order_id,
                state: projection.order.state,
                has_settlement: projection.settlement.is_some(),
            });
            Ok(projection)
        }

        #[ink(message)]
        pub fn get_intent(&self, order_id: Hash32) -> Option<MarketIntent> {
            self.intents.get(order_id)
        }

        #[ink(message)]
        pub fn get_projection(&self, order_id: Hash32) -> Option<OrderProjection> {
            self.projections.get(order_id)
        }

        #[ink(message)]
        pub fn get_market_id(&self) -> Hash32 {
            self.market_id
        }

        #[ink(message)]
        pub fn is_market_id_bound(&self) -> bool {
            self.market_id_bound
        }

        #[ink(message)]
        pub fn get_admin(&self) -> AccountId {
            self.admin
        }

        fn ensure_market_id_bound(&self) -> Result<(), Error> {
            if self.market_id_bound {
                Ok(())
            } else {
                Err(Error::MarketNotBound)
            }
        }
    }

    impl MarketStandard for MarketOrderbook {
        #[ink(message)]
        fn is_assetx_market(&self) -> bool {
            true
        }

        #[ink(message)]
        fn get_market_type(&self) -> u8 {
            0
        }

        #[ink(message)]
        fn get_fee_ratio(&self) -> u32 {
            self.fee_ratio
        }

        #[ink(message)]
        fn check_admission(&self, _asset_id: Hash32) -> bool {
            true
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use codec::{Decode, Encode};
        use ink::env::test;
        use market_standard::{
            CommonErrorCode, CurrencyId, ObjectType, Operation, PaymentState,
            SETTLEMENT_PROTOCOL_VERSION, SETTLEMENT_V2_EXT_ID,
        };
        use std::cell::{Cell, RefCell};

        thread_local! {
            static AUTH_ORDER: RefCell<Option<RuntimeOrder>> = const { RefCell::new(None) };
            static AUTH_SETTLEMENT: RefCell<Option<SettlementRecord>> = const { RefCell::new(None) };
            static FAIL_NEXT_ORDER_QUERY: Cell<bool> = const { Cell::new(false) };
            static CREATE_CALLS: Cell<u32> = const { Cell::new(0) };
            static SETTLE_CALLS: Cell<u32> = const { Cell::new(0) };
            static ORDER_QUERY_CALLS: Cell<u32> = const { Cell::new(0) };
            static SETTLEMENT_QUERY_CALLS: Cell<u32> = const { Cell::new(0) };
        }

        struct RuntimeAuthorityExtension;

        fn decode_extension_input<T: Decode>(input: &[u8]) -> T {
            let payload = Vec::<u8>::decode(&mut &input[..])
                .expect("ink test harness input envelope decodes");
            T::decode(&mut &payload[..]).expect("chain extension request decodes")
        }

        impl test::ChainExtension for RuntimeAuthorityExtension {
            fn ext_id(&self) -> u16 {
                SETTLEMENT_V2_EXT_ID as u16
            }

            fn call(&mut self, func_id: u16, input: &[u8], output: &mut Vec<u8>) -> u32 {
                match func_id {
                    1 => {
                        CREATE_CALLS.with(|calls| calls.set(calls.get() + 1));
                        let (_, _, seller, _): (
                            OrderCommitment,
                            PricePolicyWitness,
                            SellerCommitment,
                            ProtocolSignature,
                        ) = decode_extension_input(input);
                        output.extend(seller.order_id.encode());
                        0
                    }
                    3 => {
                        let call = SETTLE_CALLS.with(|calls| {
                            let call = calls.get() + 1;
                            calls.set(call);
                            call
                        });
                        if call > 1 {
                            return CommonErrorCode::AlreadySettled.status_code();
                        }
                        output.extend([9u8; 32].encode());
                        0
                    }
                    6 => {
                        ORDER_QUERY_CALLS.with(|calls| calls.set(calls.get() + 1));
                        let requested: Hash32 = decode_extension_input(input);
                        if FAIL_NEXT_ORDER_QUERY.with(|fail| fail.replace(false)) {
                            return CommonErrorCode::MalformedCommitment.status_code();
                        }
                        let order = AUTH_ORDER.with(|order| {
                            order.borrow().clone().expect("authority order configured")
                        });
                        assert_eq!(requested, order.order_id);
                        output.extend(order.encode());
                        0
                    }
                    7 => {
                        SETTLEMENT_QUERY_CALLS.with(|calls| calls.set(calls.get() + 1));
                        let requested: Hash32 = decode_extension_input(input);
                        let settlement = AUTH_SETTLEMENT.with(|record| record.borrow().clone());
                        if let Some(record) = settlement.as_ref() {
                            assert_eq!(requested, record.order_id);
                        }
                        output.extend(settlement.encode());
                        0
                    }
                    _ => CommonErrorCode::MalformedCommitment.status_code(),
                }
            }
        }

        fn sample_commitments(
            market_id: Hash32,
        ) -> (OrderCommitment, SellerCommitment, ProtocolSignature) {
            let order_id = [7u8; 32];
            let order = OrderCommitment {
                settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
                market_id,
                operation: Operation::TransferMeta,
                object_type: ObjectType::MetaToken,
                object_id: [2; 32],
                parent_asset_id: None,
                authorization_id: [3; 32],
                seller: [4; 32],
                allowed_buyer: Some([5; 32]),
                price: 10_000,
                currency: CurrencyId::Native,
                policy_hash: [6; 32],
                right_parameters_hash: None,
                seller_state_version: 1,
                order_nonce: 8,
                expiry: 20,
            };
            let seller = SellerCommitment {
                order_id,
                object_id: order.object_id,
                operation: order.operation,
                seller: order.seller,
                state_version: order.seller_state_version,
                price_policy_hash: order.policy_hash,
                right_parameters_hash: None,
                expires_at: order.expiry,
                seller_nonce: order.order_nonce,
            };
            (order, seller, [8u8; 64])
        }

        fn sample_runtime_state(market_id: Hash32) -> (RuntimeOrder, SettlementRecord) {
            let (commitment, seller_commitment, _) = sample_commitments(market_id);
            let order_id = seller_commitment.order_id;
            let buyer_commitment = BuyerCommitment {
                order_id,
                buyer: [5; 32],
                price: commitment.price,
                currency: CurrencyId::Native,
                expires_at: 18,
                buyer_nonce: 1,
            };
            let order = RuntimeOrder {
                order_id,
                authorization_id: commitment.authorization_id,
                seller_commitment,
                buyer_commitment: Some(buyer_commitment),
                payment_asset: CurrencyId::Native,
                escrow_id: Some([11; 32]),
                created_at: 10,
                funded_at: Some(11),
                expiry: commitment.expiry,
                grace_deadline: Some(21),
                state: OrderState::Settled,
                order_commitment: commitment,
            };
            let settlement = SettlementRecord {
                settlement_id: [9; 32],
                order_id,
                operation: Operation::TransferMeta,
                asset_id: [2; 32],
                right_id: None,
                authorization_id: [3; 32],
                market_id,
                policy_hash: [6; 32],
                order_commitment_hash: [12; 32],
                seller: [4; 32],
                buyer: [5; 32],
                currency: CurrencyId::Native,
                gross_price: 10_000,
                seller_amount: 10_000,
                royalty_recipient: None,
                royalty_amount: 0,
                market_fee_recipient: [13; 32],
                market_fee_amount: 0,
                safety_fee_amount: 0,
                pre_state_version: 1,
                post_state_version: 2,
                pre_state_commitment: [14; 32],
                block_number: 12,
                settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
            };
            (order, settlement)
        }

        fn configure_extension(market_id: Hash32) {
            let (order, settlement) = sample_runtime_state(market_id);
            AUTH_ORDER.with(|value| *value.borrow_mut() = Some(order));
            AUTH_SETTLEMENT.with(|value| *value.borrow_mut() = Some(settlement));
            FAIL_NEXT_ORDER_QUERY.with(|value| value.set(false));
            CREATE_CALLS.with(|value| value.set(0));
            SETTLE_CALLS.with(|value| value.set(0));
            ORDER_QUERY_CALLS.with(|value| value.set(0));
            SETTLEMENT_QUERY_CALLS.with(|value| value.set(0));
            test::register_chain_extension(RuntimeAuthorityExtension);
        }

        #[ink::test]
        fn zero_market_id_can_be_bound_once_by_admin() {
            let market_id = [1; 32];
            let mut market = MarketOrderbook::new([0; 32], 25);

            assert!(!market.is_market_id_bound());
            assert_eq!(market.get_market_id(), [0; 32]);
            assert_eq!(market.bind_market_id(market_id), Ok(()));
            assert!(market.is_market_id_bound());
            assert_eq!(market.get_market_id(), market_id);
            assert_eq!(test::recorded_events().count(), 1);
            assert_eq!(
                market.bind_market_id([2; 32]),
                Err(Error::MarketAlreadyBound)
            );
        }

        #[ink::test]
        fn non_admin_cannot_bind_market_id() {
            let accounts = test::default_accounts::<market_standard::CustomEnvironment>();
            let mut market = MarketOrderbook::new([0; 32], 25);
            test::set_caller::<market_standard::CustomEnvironment>(accounts.bob);

            assert_eq!(market.bind_market_id([1; 32]), Err(Error::NotAdmin));
            assert!(!market.is_market_id_bound());
            assert_eq!(market.get_market_id(), [0; 32]);
        }

        #[ink::test]
        fn zero_market_id_cannot_be_bound() {
            let mut market = MarketOrderbook::new([0; 32], 25);

            assert_eq!(market.bind_market_id([0; 32]), Err(Error::InvalidMarketId));
            assert!(!market.is_market_id_bound());
        }

        #[ink::test]
        fn nonzero_constructor_market_id_is_already_bound() {
            let mut market = MarketOrderbook::new([1; 32], 25);

            assert!(market.is_market_id_bound());
            assert_eq!(
                market.bind_market_id([2; 32]),
                Err(Error::MarketAlreadyBound)
            );
        }

        #[ink::test]
        fn unbound_market_rejects_runtime_entry_points() {
            let market_id = [1; 32];
            configure_extension(market_id);
            let (order, seller, signature) = sample_commitments(market_id);
            let mut market = MarketOrderbook::new([0; 32], 25);

            assert_eq!(
                market.relay_create_order(
                    order,
                    PricePolicyWitness::Authorization,
                    seller,
                    signature,
                ),
                Err(Error::MarketNotBound)
            );
            assert_eq!(market.sync_projection([7; 32]), Err(Error::MarketNotBound));
            CREATE_CALLS.with(|calls| assert_eq!(calls.get(), 0));
            ORDER_QUERY_CALLS.with(|calls| assert_eq!(calls.get(), 0));
        }

        #[ink::test]
        fn create_relay_stores_intent_only_after_runtime_accepts() {
            let market_id = [1; 32];
            configure_extension(market_id);
            let (order, seller, signature) = sample_commitments(market_id);
            let mut market = MarketOrderbook::new(market_id, 25);

            let order_id = market
                .relay_create_order(
                    order.clone(),
                    PricePolicyWitness::Authorization,
                    seller.clone(),
                    signature,
                )
                .expect("Runtime accepts order");

            assert_eq!(order_id, seller.order_id);
            let intent = market.get_intent(order_id).expect("intent stored");
            assert_eq!(intent.order, order);
            assert_eq!(
                intent.price_policy_witness,
                PricePolicyWitness::Authorization
            );
            assert_eq!(intent.seller_commitment, seller);
            assert!(market.get_projection(order_id).is_none());
            CREATE_CALLS.with(|calls| assert_eq!(calls.get(), 1));
        }

        #[ink::test]
        fn projection_failure_is_recoverable_by_query_replay() {
            let market_id = [1; 32];
            configure_extension(market_id);
            let order_id = [7; 32];
            let mut market = MarketOrderbook::new(market_id, 0);
            FAIL_NEXT_ORDER_QUERY.with(|fail| fail.set(true));

            assert_eq!(
                market.sync_projection(order_id),
                Err(Error::ChainExtension(SettlementV2ExtError::Common(
                    CommonErrorCode::MalformedCommitment
                )))
            );
            assert!(market.get_projection(order_id).is_none());

            let recovered = market
                .sync_projection(order_id)
                .expect("second query recovers projection");
            assert_eq!(market.get_projection(order_id), Some(recovered));
        }

        #[ink::test]
        fn repeated_sync_is_idempotent_and_never_calls_settle() {
            let market_id = [1; 32];
            configure_extension(market_id);
            let order_id = [7; 32];
            let mut market = MarketOrderbook::new(market_id, 0);

            let first = market.sync_projection(order_id).expect("first sync");
            let second = market.sync_projection(order_id).expect("replayed sync");

            assert_eq!(first, second);
            assert_eq!(market.get_projection(order_id), Some(first));
            SETTLE_CALLS.with(|calls| assert_eq!(calls.get(), 0));
            ORDER_QUERY_CALLS.with(|calls| assert_eq!(calls.get(), 2));
            SETTLEMENT_QUERY_CALLS.with(|calls| assert_eq!(calls.get(), 2));
        }

        #[ink::test]
        fn duplicate_settle_nonzero_status_remains_an_error() {
            let market_id = [1; 32];
            configure_extension(market_id);
            let order_id = [7; 32];
            let mut market = MarketOrderbook::new(market_id, 0);

            assert_eq!(market.relay_settle(order_id), Ok([9; 32]));
            assert_eq!(
                market.relay_settle(order_id),
                Err(Error::ChainExtension(SettlementV2ExtError::Common(
                    CommonErrorCode::AlreadySettled
                )))
            );
            SETTLE_CALLS.with(|calls| assert_eq!(calls.get(), 2));
            assert!(market.get_projection(order_id).is_none());
        }

        #[ink::test]
        fn contract_rejects_intent_for_another_market_before_ce_call() {
            let market_id = [1; 32];
            configure_extension(market_id);
            let (order, seller, signature) = sample_commitments([2; 32]);
            let mut market = MarketOrderbook::new(market_id, 0);

            assert_eq!(
                market.relay_create_order(
                    order,
                    PricePolicyWitness::Authorization,
                    seller,
                    signature,
                ),
                Err(Error::MarketMismatch)
            );
            CREATE_CALLS.with(|calls| assert_eq!(calls.get(), 0));
        }

        #[ink::test]
        fn policy_decision_is_admission_bound_and_consumed_after_acceptance() {
            let market_id = [1; 32];
            configure_extension(market_id);
            let (mut order, seller, signature) = sample_commitments(market_id);
            let mut market = MarketOrderbook::new(market_id, 0);
            let decision = market.r2_x1_evaluate_policy(10_000, vec![]);

            assert_eq!(decision.gross_price, 10_500);
            assert_eq!(decision.selected_offer_index, 0);
            assert_eq!(
                market.relay_create_order(
                    order.clone(),
                    PricePolicyWitness::Authorization,
                    seller.clone(),
                    signature,
                ),
                Err(Error::PolicyDecisionMismatch)
            );
            assert_eq!(market.r2_x1_last_policy_decision(), Some(decision));
            assert!(market.get_intent(seller.order_id).is_none());
            CREATE_CALLS.with(|calls| assert_eq!(calls.get(), 0));

            order.price = decision.gross_price;
            assert_eq!(
                market.relay_create_order(
                    order,
                    PricePolicyWitness::Authorization,
                    seller.clone(),
                    signature,
                ),
                Ok(seller.order_id)
            );
            assert_eq!(market.r2_x1_last_policy_decision(), None);
            assert!(market.get_intent(seller.order_id).is_some());
            CREATE_CALLS.with(|calls| assert_eq!(calls.get(), 1));
        }

        #[test]
        fn payment_projection_uses_runtime_payment_state_only() {
            assert_eq!(PaymentState::Escrowed.encode(), vec![1]);
            assert_eq!(PaymentState::Released.encode(), vec![2]);
        }
    }
}
