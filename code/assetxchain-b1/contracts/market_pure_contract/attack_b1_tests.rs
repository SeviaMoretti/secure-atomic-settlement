// attack_b1_tests.rs — 对论文对照组 B1（DT-SC-adapted，纯合约路径）的攻击性验证
//
// 对照组属性：B1 将市场策略以及权属、授权、付款、订单终态和证据全部维护在合约侧，
// 不经过运行时统一边界（论文 G1/G2 的对照对象）。
//
// 攻击点（与论文表 8 / E7 攻击面映射）：
//   A1 未授权调用者抢先结算（G2 无调用者权威检查）——任意旁路账户触发 Funded 订单结算
//   A2 未授权调用者强制退款（G2 无调用者权威检查）——宽限期后任意账户强制 refund，中断合法交易（DoS）
//   A3 未授权调用者过期清理（G2）——到期后任意账户触发 expire
//   A4 结算路径非原子窗口（G1）——中途失败（abort 注入）后观察权属/托管/订单状态是否部分写入
//   A5 非 owner 创建授权（防御面核验）——必须被拒绝
//   A6 重复结算（防御面核验）——第二次必须被拒绝
//
// 依赖 B1 lib.rs mod tests 内已有的 keypair/签名/fund 等测试辅助函数（include 方式接入）。

// ========================================================================
// 场景构造：注册资产 → 市场 → 授权 → relayer 创建订单 → buyer 托管付款
// 返回 (order_id, escrow_id, asset_id, market_id, auth_id)
// ========================================================================
fn b1_setup_transfer_meta(
    contract: &mut MarketPureContract,
    seller: &Keypair,
    buyer: &Keypair,
    operator: &Keypair,
    relayer: &Keypair,
) -> (Hash32, Hash32, Hash32, Hash32, Hash32) {
    set_caller(account(seller));
    set_block(1);
    let asset_id = [0xc1; 32];
    contract
        .register_meta(meta_registration(account(seller), asset_id))
        .unwrap();
    let market_id = settlement_market(contract, account(operator), CAP_CREATE_META_ORDER, 100);
    let policy = settlement_policy(TRANSFER_META_PRICE, 100);
    let auth_id = contract
        .create_transfer_meta_authorization(
            asset_id,
            market_id,
            TransferMetaAuthorizationTerms {
                price_policy: policy,
                allowed_buyer: Some(account(buyer)),
                expiry: 20,
                nonce: 0,
            },
        )
        .unwrap();
    let order = OrderCommitment {
        settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
        market_id,
        operation: MarketOperation::TransferMeta,
        object_type: ObjectType::MetaToken,
        object_id: asset_id,
        parent_asset_id: None,
        authorization_id: auth_id,
        seller: protocol_account(seller),
        allowed_buyer: Some(protocol_account(buyer)),
        price: TRANSFER_META_PRICE,
        currency: CurrencyId::Native,
        policy_hash: contract.hash_encoded(&policy),
        right_parameters_hash: None,
        seller_state_version: 1,
        order_nonce: 0,
        expiry: 20,
    };
    let (seller_commitment, seller_signature) = seller_commitment(contract, &order, seller);
    set_caller(account(relayer));
    let order_id = contract
        .create_order(
            order.clone(),
            PricePolicyWitness::Authorization,
            seller_commitment,
            seller_signature,
        )
        .unwrap();
    let escrow_id = fund(contract, &order, buyer, 0);
    (order_id, escrow_id, asset_id, market_id, auth_id)
}

// ========================================================================
// A1 — 未授权调用者抢先结算（G2 无调用者权威检查）
// 攻击者 eve 完全不参与交易，仅持有订单号即可触发结算。
// ========================================================================
#[ink::test]
fn attack_b1_unauthorized_settler_preempts() {
    let seller = keypair(1);
    let buyer = keypair(2);
    let operator = keypair(3);
    let relayer = keypair(4);
    let eve = keypair(9);
    initialize_balances(&[
        account(&seller),
        account(&buyer),
        account(&operator),
        account(&relayer),
        account(&eve),
    ]);
    set_caller(account(&seller));
    set_block(1);
    let mut contract = MarketPureContract::new_with_genesis_hash([0x99; 32]);
    let (order_id, _escrow, asset_id, _market, _auth) =
        b1_setup_transfer_meta(&mut contract, &seller, &buyer, &operator, &relayer);

    // 攻击：未参与交易的 eve 直接结算
    set_caller(account(&eve));
    let r = contract.settle(order_id);
    assert!(
        r.is_ok(),
        "B1 缺陷：未授权调用者（非买方/非中继/非卖方）竟可抢先结算 Funded 订单"
    );
    let order = contract.get_order(order_id).unwrap();
    assert_eq!(order.state, OrderState::Settled, "订单被攻击者触发进入 Settled");
    assert_eq!(
        contract.get_meta(asset_id).unwrap().owner,
        account(&buyer),
        "权属仍转移给买方（结果合法，但触发者无权限）"
    );
    println!(
        "[A1] attacker_settle=OK (caller=eve, unauthorized) order=Settled meta_owner=buyer"
    );
}

// ========================================================================
// A2 — 未授权调用者强制退款（G2 / DoS）
// 宽限期后，攻击者 eve 可强制 refund 任何 Funded 订单，中断买卖双方的合法结算。
// ========================================================================
#[ink::test]
fn attack_b1_unauthorized_refunder_dos() {
    let seller = keypair(1);
    let buyer = keypair(2);
    let operator = keypair(3);
    let relayer = keypair(4);
    let eve = keypair(9);
    initialize_balances(&[
        account(&seller),
        account(&buyer),
        account(&operator),
        account(&relayer),
        account(&eve),
    ]);
    set_caller(account(&seller));
    set_block(1);
    let mut contract = MarketPureContract::new_with_genesis_hash([0x99; 32]);
    let (order_id, escrow_id, _asset, _market, _auth) =
        b1_setup_transfer_meta(&mut contract, &seller, &buyer, &operator, &relayer);
    assert_eq!(
        contract.get_order(order_id).unwrap().state,
        OrderState::Funded
    );

    // 宽限期结束后（fund 在 block 1，grace=10 → deadline=11），攻击者 eve 强制退款
    set_block(21);
    set_caller(account(&eve));
    let r = contract.refund_funded(order_id);
    assert!(
        r.is_ok(),
        "B1 缺陷：未授权调用者可在宽限期后强制退款合法订单（DoS）"
    );
    assert_eq!(
        contract.get_order(order_id).unwrap().state,
        OrderState::Refunded,
        "订单被攻击者强制置为 Refunded，合法结算机会被破坏"
    );
    assert_eq!(
        contract.get_escrow(escrow_id).unwrap().state,
        PaymentState::Refunded
    );
    println!("[A2] attacker_refund=OK (caller=eve, unauthorized) order=Refunded escrow=Refunded");
}

// ========================================================================
// A3 — 未授权调用者过期清理（G2）
// 到期后任意账户可触发 expire（低危清理入口，仍属无调用者权限）。
// ========================================================================
#[ink::test]
fn attack_b1_unauthorized_expirer() {
    let seller = keypair(1);
    let buyer = keypair(2);
    let operator = keypair(3);
    let relayer = keypair(4);
    let eve = keypair(9);
    initialize_balances(&[
        account(&seller),
        account(&buyer),
        account(&operator),
        account(&relayer),
        account(&eve),
    ]);
    set_caller(account(&seller));
    set_block(1);
    let mut contract = MarketPureContract::new_with_genesis_hash([0x99; 32]);
    // 构造 Open 订单但不 fund
    set_caller(account(&seller));
    set_block(1);
    let asset_id = [0xc1; 32];
    contract
        .register_meta(meta_registration(account(&seller), asset_id))
        .unwrap();
    let market_id = settlement_market(&mut contract, account(&operator), CAP_CREATE_META_ORDER, 100);
    let policy = settlement_policy(TRANSFER_META_PRICE, 100);
    let auth_id = contract
        .create_transfer_meta_authorization(
            asset_id,
            market_id,
            TransferMetaAuthorizationTerms {
                price_policy: policy,
                allowed_buyer: Some(account(&buyer)),
                expiry: 20,
                nonce: 0,
            },
        )
        .unwrap();
    let order = OrderCommitment {
        settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
        market_id,
        operation: MarketOperation::TransferMeta,
        object_type: ObjectType::MetaToken,
        object_id: asset_id,
        parent_asset_id: None,
        authorization_id: auth_id,
        seller: protocol_account(&seller),
        allowed_buyer: Some(protocol_account(&buyer)),
        price: TRANSFER_META_PRICE,
        currency: CurrencyId::Native,
        policy_hash: contract.hash_encoded(&policy),
        right_parameters_hash: None,
        seller_state_version: 1,
        order_nonce: 0,
        expiry: 20,
    };
    let (sc, ss) = seller_commitment(&contract, &order, &seller);
    set_caller(account(&relayer));
    let order_id = contract
        .create_order(order, PricePolicyWitness::Authorization, sc, ss)
        .unwrap();
    assert_eq!(
        contract.get_order(order_id).unwrap().state,
        OrderState::Open
    );

    // 到期后 eve 触发过期
    set_block(21);
    set_caller(account(&eve));
    let r = contract.expire_open(order_id);
    assert!(r.is_ok(), "B1：到期订单可被任意未授权账户触发过期");
    assert_eq!(
        contract.get_order(order_id).unwrap().state,
        OrderState::Expired
    );
    println!("[A3] attacker_expire=OK (caller=eve, unauthorized) order=Expired");
}

// ========================================================================
// A4 — 结算路径非原子窗口（G1）
// 结算中途失败（abort 注入，模拟执行中异常）后，观察权属、托管与订单状态是否部分写入。
// 测试环境下 revert_on_error 直接返回 Err（不触发平台级回滚），
// 因此可观测到纯合约结算路径自身的写集是否原子。
// ========================================================================
#[ink::test]
fn attack_b1_settle_abort_leaves_partial_writes() {
    let seller = keypair(1);
    let buyer = keypair(2);
    let operator = keypair(3);
    let relayer = keypair(4);
    let eve = keypair(9);
    initialize_balances(&[
        account(&seller),
        account(&buyer),
        account(&operator),
        account(&relayer),
        account(&eve),
    ]);
    set_caller(account(&seller));
    set_block(1);
    let mut contract = MarketPureContract::new_with_genesis_hash([0x99; 32]);
    let (order_id, escrow_id, asset_id, _market, auth_id) =
        b1_setup_transfer_meta(&mut contract, &seller, &buyer, &operator, &relayer);

    let seller_account = account(&seller);
    let buyer_account = account(&buyer);
    let seller_before =
        test::get_account_balance::<ink::env::DefaultEnvironment>(seller_account).unwrap();
    let escrow_id_storage = escrow_id;

    // 注入 TransferMeta 结算的中途失败（abort 点在权属写入之后、授权消费之前）
    MarketPureContract::set_test_abort_operation(Some(MarketOperation::TransferMeta));
    set_caller(account(&eve));
    let r = contract.settle(order_id);
    assert!(r.is_err(), "abort 注入使结算返回失败");
    MarketPureContract::set_test_abort_operation(None);

    // 观察撕裂：失败后哪些状态已经部分写入
    let order_after = contract.get_order(order_id).unwrap();
    let escrow_after = contract.get_escrow(escrow_id_storage).unwrap();
    let meta_after = contract.get_meta(asset_id).unwrap();
    let auth_after = contract.get_transfer_meta_authorization(auth_id).unwrap();
    let seller_after =
        test::get_account_balance::<ink::env::DefaultEnvironment>(seller_account).unwrap();

    println!(
        "[A4] after_failed_settle: order.state={:?} escrow.state={:?} meta.owner_is_buyer={} auth.status={:?} seller_delta={}",
        order_after.state,
        escrow_after.state,
        meta_after.owner == buyer_account,
        auth_after.status,
        seller_after.saturating_sub(seller_before)
    );
    // 撕裂判据（修正版）：结算返回 Err，但（a）权属已转移 &&（b）付款已释放到卖方 &&（c）订单仍非终态
    let payment_released = seller_after > seller_before;
    let torn = meta_after.owner == buyer_account
        && payment_released
        && order_after.state == OrderState::Funded;
    println!(
        "[A4] non_atomic_write_window={} (ownership_moved={}, payment_released={}, order_still_funded={})",
        torn,
        meta_after.owner == buyer_account,
        payment_released,
        order_after.state == OrderState::Funded
    );
    // 该窗口本身是观察结论：B1 结算写集在单路径内不具备自证原子性，
    // 正确性依赖平台级调用回滚（链上 revert 语义），与 G3 的运行时统一边界形成对照。
    let _ = torn;
}

// ========================================================================
// A5 — 非 owner 创建授权（防御面核验）
// 攻击者 eve 试图为他人资产创建转售授权：必须被拒绝。
// ========================================================================
#[ink::test]
fn attack_b1_non_owner_cannot_create_auth() {
    let seller = keypair(1);
    let buyer = keypair(2);
    let operator = keypair(3);
    let eve = keypair(9);
    initialize_balances(&[
        account(&seller),
        account(&buyer),
        account(&operator),
        account(&eve),
    ]);
    set_caller(account(&seller));
    set_block(1);
    let mut contract = MarketPureContract::new_with_genesis_hash([0x99; 32]);
    set_caller(account(&seller));
    set_block(1);
    let asset_id = [0xc1; 32];
    contract
        .register_meta(meta_registration(account(&seller), asset_id))
        .unwrap();
    let market_id = settlement_market(&mut contract, account(&operator), CAP_CREATE_META_ORDER, 100);

    // eve（非 owner）试图授权转售 seller 的资产
    set_caller(account(&eve));
    let r = contract.create_transfer_meta_authorization(
        asset_id,
        market_id,
        TransferMetaAuthorizationTerms {
            price_policy: settlement_policy(TRANSFER_META_PRICE, 100),
            allowed_buyer: Some(account(&eve)),
            expiry: 20,
            nonce: 0,
        },
    );
    assert!(
        r.is_err(),
        "非 owner 创建授权必须被拒绝（B1 授权创建有 owner 检查）"
    );
    println!("[A5] non_owner_create_auth=REJECTED (defense ok)");
}

// ========================================================================
// A6 — 重复结算（防御面核验）
// 同一订单二次结算必须被拒绝；结算记录唯一。
// ========================================================================
#[ink::test]
fn attack_b1_replay_settle_rejected() {
    let seller = keypair(1);
    let buyer = keypair(2);
    let operator = keypair(3);
    let relayer = keypair(4);
    let eve = keypair(9);
    initialize_balances(&[
        account(&seller),
        account(&buyer),
        account(&operator),
        account(&relayer),
        account(&eve),
    ]);
    set_caller(account(&seller));
    set_block(1);
    let mut contract = MarketPureContract::new_with_genesis_hash([0x99; 32]);
    let (order_id, _escrow, _asset, _market, _auth) =
        b1_setup_transfer_meta(&mut contract, &seller, &buyer, &operator, &relayer);

    // 首次结算（买方发起）
    set_caller(account(&buyer));
    contract.settle(order_id).unwrap();
    assert_eq!(
        contract.get_order(order_id).unwrap().state,
        OrderState::Settled
    );
    // 攻击者 eve 重放
    set_caller(account(&eve));
    let r2 = contract.settle(order_id);
    assert!(
        r2.is_err(),
        "重复结算必须被拒绝（B1 有 AlreadySettled 防御）"
    );
    println!("[A6] replay_settle=REJECTED (defense ok)");
}
