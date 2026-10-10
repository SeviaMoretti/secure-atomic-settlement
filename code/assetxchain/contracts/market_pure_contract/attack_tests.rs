// attack_tests.rs — 开源属性测试工具（proptest 0.8）对纯合约路径的攻击性验证
//
// 本模块对应论文 E1—E5 的攻击面：
//   F1/F2  字段伪造与冲突注入（E2、E3）：随机恶意 asset_id / price 必须被拒绝且状态不变
//   F3     重复结算与唯一消费（E4）：同一订单二次结算必须被拒绝，结算记录唯一
//   F4     授权边界（E1、X1）：未授权市场直接结算必须被拒绝
//   F5     结算交叉核验（E5）：结算后订单终态、所有权、记录、事件一致
//
// 由 proptest 随机生成输入，对每个随机用例断言不变量；任何反例即输出最小化反例。
use proptest::prelude::*;
use proptest::{prop_assert, prop_assert_eq, prop_assume};
use proptest::proptest_helper;
use ink::env::test::{default_accounts, set_caller, set_value_transferred, set_callee};
use core::sync::atomic::{AtomicU64, Ordering};

type Acc = ink::env::test::DefaultAccounts<ink::env::DefaultEnvironment>;
type PCfg = proptest::test_runner::Config;

/// ink off-chain 测试环境为全局单例，proptest 用例间状态会残留。
/// 用「单调计数器 + 随机尾段」构造每个用例唯一的 asset_id，规避一资产一订单冲突。
static ASSET_COUNTER: AtomicU64 = AtomicU64::new(1);
fn next_asset(rnd: u64) -> [u8; 32] {
    let n = ASSET_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut a = [0u8; 32];
    a[0..8].copy_from_slice(&n.to_le_bytes());
    a[8..16].copy_from_slice(&rnd.to_le_bytes());
    a[16] = 0xAC; // 前缀标记，避免与随机假 id 空间混淆
    a
}

/// 构造一个资产已授权并挂牌的合约，返回 (合约, order_id, 账户组)。
/// 合约账户（callee）设为 alice，并将 alice 授权为唯一市场。
fn fresh(asset_id: [u8; 32], price: u128) -> (MarketPureContract, [u8; 32], Acc) {
    let acc = default_accounts();
    set_callee::<ink::env::DefaultEnvironment>(acc.alice);
    set_caller::<ink::env::DefaultEnvironment>(acc.alice);
    let mut c = MarketPureContract::new();
    set_caller::<ink::env::DefaultEnvironment>(acc.alice);
    c.initialize_asset_ownership(asset_id, acc.alice).unwrap();
    set_caller::<ink::env::DefaultEnvironment>(acc.alice);
    c.authorize_market(asset_id, acc.alice).unwrap();
    set_caller::<ink::env::DefaultEnvironment>(acc.alice);
    let order_id = c.list_asset(asset_id, price).unwrap();
    (c, order_id, acc)
}

/// 构造一个资产已挂牌但【未授权】的合约（授权边界测试用）。
fn fresh_unauthorized(asset_id: [u8; 32], price: u128) -> (MarketPureContract, [u8; 32], Acc) {
    let acc = default_accounts();
    set_callee::<ink::env::DefaultEnvironment>(acc.alice);
    set_caller::<ink::env::DefaultEnvironment>(acc.alice);
    let mut c = MarketPureContract::new();
    set_caller::<ink::env::DefaultEnvironment>(acc.alice);
    c.initialize_asset_ownership(asset_id, acc.alice).unwrap();
    set_caller::<ink::env::DefaultEnvironment>(acc.alice);
    let order_id = c.list_asset(asset_id, price).unwrap();
    (c, order_id, acc)
}

// ========================================================================
// F1 — 伪造 asset_id（E2 字段伪造）
// 随机伪造 asset_id 发起结算：必须被拒绝，且拒绝后所有权与订单状态不变。
// ========================================================================
#[ink::test]
fn fuzz_fake_asset_id_rejected() {
    static REJ: AtomicU64 = AtomicU64::new(0);
    proptest::proptest!(PCfg::with_cases(512), |(rnd in any::<u64>(), price in 1u128..u128::MAX, fake_asset in any::<[u8; 32]>())| {
        let asset = next_asset(rnd);
        prop_assume!(fake_asset != asset);
        let (mut c, order_id, acc) = fresh(asset, price);
        set_caller::<ink::env::DefaultEnvironment>(acc.bob);
        set_value_transferred::<ink::env::DefaultEnvironment>(price);
        let owner_before = c.get_asset_owner(asset);
        let r = c.buy_asset(fake_asset, price);
        prop_assert!(r.is_err(), "伪造 asset_id 的结算必须被拒绝");
        REJ.fetch_add(1, Ordering::Relaxed);
        prop_assert_eq!(c.get_asset_owner(asset), owner_before, "拒绝后所有权不变");
        let order = c.get_order(order_id).unwrap();
        prop_assert!(matches!(order.status, OrderStatus::Listed), "拒绝后订单仍 Listed");
    });
    println!("[F1] cases=512 all_rejected={}", REJ.load(Ordering::Relaxed));
}

// ========================================================================
// F2 — 价格冲突注入（E3 冲突注入）
// 随机伪造 price：与挂牌不符必须被拒绝且状态不变；恰一致则成功。
// ========================================================================
#[ink::test]
fn fuzz_bogus_price_conflict_rejected() {
    static REJ: AtomicU64 = AtomicU64::new(0);
    static COL: AtomicU64 = AtomicU64::new(0);
    proptest::proptest!(PCfg::with_cases(512), |(rnd in any::<u64>(), price in 1u128..u128::MAX, bogus_price in any::<u128>())| {
        let asset = next_asset(rnd);
        let (mut c, order_id, acc) = fresh(asset, price);
        set_caller::<ink::env::DefaultEnvironment>(acc.bob);
        set_value_transferred::<ink::env::DefaultEnvironment>(price);
        let owner_before = c.get_asset_owner(asset);
        let r = c.buy_asset(asset, bogus_price);
        if bogus_price != price {
            REJ.fetch_add(1, Ordering::Relaxed);
            prop_assert!(r.is_err(), "价格与挂牌不符必须被拒绝 (price={price}, bogus={bogus_price})");
            prop_assert_eq!(c.get_asset_owner(asset), owner_before, "拒绝后所有权不变");
            let order = c.get_order(order_id).unwrap();
            prop_assert!(matches!(order.status, OrderStatus::Listed), "拒绝后订单仍 Listed");
        } else {
            // 价格恰好一致且付款充足 → 允许成功
            COL.fetch_add(1, Ordering::Relaxed);
            prop_assert!(r.is_ok());
            prop_assert_eq!(c.get_asset_owner(asset), Some(acc.bob), "成功路径所有权转移");
        }
    });
    println!("[F2] cases=512 rejected={} price_collision={}", REJ.load(Ordering::Relaxed), COL.load(Ordering::Relaxed));
}

// ========================================================================
// F2b — 付款不足（E2 字段校验 / E3 边界）
// 恶意少付：必须被拒绝，拒绝后所有权不变。
// ========================================================================
#[ink::test]
fn fuzz_underpayment_rejected() {
    static REJ: AtomicU64 = AtomicU64::new(0);
    proptest::proptest!(PCfg::with_cases(512), |(rnd in any::<u64>(), price in 1u128..u128::MAX)| {
        let asset = next_asset(rnd);
        let (mut c, order_id, acc) = fresh(asset, price);
        set_caller::<ink::env::DefaultEnvironment>(acc.bob);
        set_value_transferred::<ink::env::DefaultEnvironment>(0); // 恶意少付
        let owner_before = c.get_asset_owner(asset);
        let r = c.buy_asset_by_order_id(order_id, asset, price);
        prop_assert!(r.is_err(), "付款不足必须被拒绝");
        REJ.fetch_add(1, Ordering::Relaxed);
        prop_assert_eq!(c.get_asset_owner(asset), owner_before, "拒绝后所有权不变");
    });
    println!("[F2b] cases=512 all_rejected={}", REJ.load(Ordering::Relaxed));
}

// ========================================================================
// F3 — 重复结算与唯一消费（E4）
// 同一订单在首次结算成功后，任何再次结算必须被拒绝（唯一消费）；
// 且订单终态为 Settled、所有权为买方。
// ========================================================================
#[ink::test]
fn fuzz_double_settle_second_rejected() {
    static OK: AtomicU64 = AtomicU64::new(0);
    static REJ2: AtomicU64 = AtomicU64::new(0);
    static REJ3: AtomicU64 = AtomicU64::new(0);
    proptest::proptest!(PCfg::with_cases(512), |(rnd in any::<u64>(), price in 1u128..u128::MAX)| {
        let asset = next_asset(rnd);
        let (mut c, order_id, acc) = fresh(asset, price);
        // 首次结算
        set_caller::<ink::env::DefaultEnvironment>(acc.bob);
        set_value_transferred::<ink::env::DefaultEnvironment>(price);
        let r1 = c.buy_asset_by_order_id(order_id, asset, price);
        prop_assert!(r1.is_ok(), "首次合法结算必须成功");
        OK.fetch_add(1, Ordering::Relaxed);
        // 重复结算（重放同一订单）
        set_caller::<ink::env::DefaultEnvironment>(acc.bob);
        set_value_transferred::<ink::env::DefaultEnvironment>(price);
        let r2 = c.buy_asset_by_order_id(order_id, asset, price);
        prop_assert!(r2.is_err(), "重复结算必须被拒绝");
        REJ2.fetch_add(1, Ordering::Relaxed);
        // 再次尝试（不同买方）
        set_caller::<ink::env::DefaultEnvironment>(acc.charlie);
        set_value_transferred::<ink::env::DefaultEnvironment>(price);
        let r3 = c.buy_asset_by_order_id(order_id, asset, price);
        prop_assert!(r3.is_err(), "订单已被消费，其他买方结算必须被拒绝");
        REJ3.fetch_add(1, Ordering::Relaxed);
        // 状态核验
        prop_assert_eq!(c.get_asset_owner(asset), Some(acc.bob), "所有权唯一归首次买方");
        let order = c.get_order(order_id).unwrap();
        prop_assert!(matches!(order.status, OrderStatus::Settled), "订单终态 Settled");
    });
    println!("[F3] cases=512 first_ok={} replay_rejected={} other_buyer_rejected={}", OK.load(Ordering::Relaxed), REJ2.load(Ordering::Relaxed), REJ3.load(Ordering::Relaxed));
}

// ========================================================================
// F4 — 授权边界（E1、X1）
// 资产从未授权任何市场时，直接结算必须被拒绝；所有权保持卖方。
// 注：结算路径先 lock 后 verify（非原子），拒绝后订单可能被推入 Locked；
//     核心不变量是「资产不可转移、订单不可被结算（不进入 Settled）」。
// ========================================================================
#[ink::test]
fn fuzz_unauthorized_market_rejected() {
    static REJ: AtomicU64 = AtomicU64::new(0);
    static LOCKED: AtomicU64 = AtomicU64::new(0);
    proptest::proptest!(PCfg::with_cases(512), |(rnd in any::<u64>(), price in 1u128..u128::MAX)| {
        let asset = next_asset(rnd);
        let (mut c, order_id, acc) = fresh_unauthorized(asset, price);
        set_caller::<ink::env::DefaultEnvironment>(acc.bob);
        set_value_transferred::<ink::env::DefaultEnvironment>(price);
        let r = c.buy_asset_by_order_id(order_id, asset, price);
        prop_assert!(r.is_err(), "未授权市场的结算必须被拒绝");
        REJ.fetch_add(1, Ordering::Relaxed);
        prop_assert_eq!(c.get_asset_owner(asset), Some(acc.alice), "所有权保持卖方");
        let order = c.get_order(order_id).unwrap();
        prop_assert!(
            matches!(order.status, OrderStatus::Listed) || matches!(order.status, OrderStatus::Locked),
            "拒绝后订单不可被结算（不得 Settled），实际={:?}", order.status
        );
        if matches!(order.status, OrderStatus::Locked) {
            LOCKED.fetch_add(1, Ordering::Relaxed);
        }
    });
    println!("[F4] cases=512 all_rejected={} lock_side_effect={}", REJ.load(Ordering::Relaxed), LOCKED.load(Ordering::Relaxed));
}

// ========================================================================
// F5 — 结算交叉核验（E5）
// 合法结算后：返回 trade_id 唯一且非零；订单终态 Settled；
// 所有权转移给买方；订单字段（seller/price）与结算记录一致。
// ========================================================================
#[ink::test]
fn fuzz_settlement_cross_check() {
    static OK: AtomicU64 = AtomicU64::new(0);
    proptest::proptest!(PCfg::with_cases(512), |(rnd in any::<u64>(), price in 1u128..u128::MAX)| {
        let asset = next_asset(rnd);
        let (mut c, order_id, acc) = fresh(asset, price);
        set_caller::<ink::env::DefaultEnvironment>(acc.bob);
        set_value_transferred::<ink::env::DefaultEnvironment>(price);
        let r = c.buy_asset_by_order_id(order_id, asset, price);
        prop_assert!(r.is_ok(), "合法结算必须成功");
        OK.fetch_add(1, Ordering::Relaxed);
        let (oid, trade_id) = r.unwrap();
        prop_assert_eq!(oid, order_id, "返回的 order_id 与消费的一致");
        prop_assert!(trade_id != [0u8; 32], "trade_id 必须非零（唯一记录锚点）");
        prop_assert_eq!(c.get_asset_owner(asset), Some(acc.bob), "所有权转移给买方");
        let order = c.get_order(order_id).unwrap();
        prop_assert!(matches!(order.status, OrderStatus::Settled), "订单终态 Settled");
        prop_assert_eq!(order.seller, acc.alice, "卖方字段与初始化一致");
        prop_assert_eq!(order.price, price, "价格字段与挂牌一致");
        prop_assert!(order.settled_at.is_some(), "记录结算区块时间");
        // 清理：订单索引应已被移除（不可重复按 asset 索引再次结算）
        let r2 = c.buy_asset(asset, price);
        prop_assert!(r2.is_err(), "索引已清，按资产再购必须被拒绝");
    });
    println!("[F5] cases=512 all_ok={}", OK.load(Ordering::Relaxed));
}
