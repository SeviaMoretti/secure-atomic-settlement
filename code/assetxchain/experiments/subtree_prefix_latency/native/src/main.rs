use sp_core::{
    hashing::{blake2_256, keccak_256},
    Blake2Hasher, H256,
};
use sp_trie::{
    generate_trie_proof, verify_trie_proof, LayoutV1, MemoryDB, Trie, TrieDBBuilder,
    TrieDBMutBuilder, TrieMut,
};
use std::{hint::black_box, time::Instant};
type L = LayoutV1<Blake2Hasher>;
type Item = (Vec<u8>, Option<Vec<u8>>);
struct Tree {
    db: MemoryDB<Blake2Hasher>,
    root: H256,
}
impl Tree {
    fn new() -> Self {
        Self {
            db: MemoryDB::default(),
            root: H256::default(),
        }
    }
    fn fill(&mut self, rows: &[(Vec<u8>, Vec<u8>)]) {
        let mut t = TrieDBMutBuilder::<L>::new(&mut self.db, &mut self.root).build();
        for (k, v) in rows {
            t.insert(k, v).unwrap();
        }
    }
}
#[derive(Clone)]
struct Part {
    root: H256,
    proof: Vec<Vec<u8>>,
    items: Vec<Item>,
    tree: usize,
}
#[derive(Clone)]
struct Query {
    parts: Vec<Part>,
}
fn valid(q: &Query) -> bool {
    q.parts
        .iter()
        .all(|p| verify_trie_proof::<L, _, _, _>(&p.root, &p.proof, p.items.iter()).is_ok())
}
fn read_valid(q: &Query, trees: &[Tree]) -> bool {
    q.parts.iter().all(|p| {
        let t = TrieDBBuilder::<L>::new(&trees[p.tree].db, &p.root).build();
        p.items.iter().all(|(k, v)| t.get(k).unwrap() == *v)
    })
}
fn id(seed: u64, kind: u8, a: usize, b: usize) -> Vec<u8> {
    let mut x = seed.to_le_bytes().to_vec();
    x.push(kind);
    x.extend_from_slice(&(a as u64).to_le_bytes());
    x.extend_from_slice(&(b as u64).to_le_bytes());
    blake2_256(&x).to_vec()
}
fn key(asset: &[u8], cert: Option<&[u8]>, hashed: bool) -> Vec<u8> {
    if let Some(c) = cert {
        let mut k = asset.to_vec();
        k.extend_from_slice(c);
        if hashed {
            keccak_256(&k).to_vec()
        } else {
            k
        }
    } else {
        let mut k = b"assets/".to_vec();
        k.extend_from_slice(asset);
        k
    }
}
fn value(asset: &[u8], kind: u8, seed: u64, a: usize, b: usize) -> Vec<u8> {
    // Synthetic fixed-size record, identical across all four layouts.
    let mut v = asset.to_vec();
    v.extend_from_slice(&id(seed, kind, a, b));
    v.resize(256, kind);
    v
}
fn part(tree: &Tree, tree_no: usize, items: Vec<Item>) -> Part {
    let keys: Vec<Vec<u8>> = items.iter().map(|x| x.0.clone()).collect();
    let proof = generate_trie_proof::<L, _, _, _>(&tree.db, tree.root, keys.iter()).unwrap();
    Part {
        root: tree.root,
        proof,
        items,
        tree: tree_no,
    }
}
struct Case {
    mode: String,
    trees: Vec<Tree>,
    queries: Vec<Query>,
}
fn prepare(n: usize, k: usize, seed: u64, mode: &str, work: &str) -> Case {
    let dual = mode.starts_with("dual");
    let hashed = mode.ends_with("hash");
    let mut rows = vec![vec![], vec![]];
    for a in 0..n {
        let aid = id(seed, 0, a, 0);
        rows[0].push((key(&aid, None, hashed), value(&aid, 0, seed, a, 0)));
        for b in 0..k {
            let cid = id(seed, 1, a, b);
            rows[if dual { 1 } else { 0 }]
                .push((key(&aid, Some(&cid), hashed), value(&aid, 1, seed, a, b)));
        }
    }
    let mut trees = vec![Tree::new(), Tree::new()];
    for t in 0..2 {
        trees[t].fill(&rows[t]);
    }
    let mut queries = vec![];
    for j in 0..32 {
        let h = id(seed, 3, j, 0);
        let a = (u64::from_le_bytes(h[..8].try_into().unwrap()) as usize) % n;
        let b = (u64::from_le_bytes(h[8..16].try_into().unwrap()) as usize) % k;
        let aid = id(seed, 0, a, 0);
        let cid = id(seed, 1, a, b);
        let mut items = vec![vec![], vec![]];
        if work != "right" {
            items[0].push((key(&aid, None, hashed), Some(value(&aid, 0, seed, a, 0))));
        }
        if work != "meta" {
            items[if dual { 1 } else { 0 }].push((
                key(&aid, Some(&cid), hashed),
                Some(value(&aid, 1, seed, a, b)),
            ));
        }
        let mut parts = vec![];
        for t in 0..2 {
            if !items[t].is_empty() {
                parts.push(part(&trees[t], t, items[t].clone()));
            }
        }
        queries.push(Query { parts });
    }
    Case {
        mode: mode.into(),
        trees,
        queries,
    }
}
fn correctness(case: &Case) -> usize {
    let mut checks = 0;
    for q in &case.queries {
        assert!(valid(q));
        assert!(read_valid(q, &case.trees));
        checks += 2;
        for i in 0..q.parts.len() {
            let mut bad = q.clone();
            bad.parts[i].items[0].1.as_mut().unwrap()[0] ^= 1;
            assert!(!valid(&bad));
            checks += 1;
            let mut bad = q.clone();
            bad.parts[i].root = H256::repeat_byte(0xab);
            assert!(!valid(&bad));
            checks += 1;
            let mut bad = q.clone();
            bad.parts[i].proof.clear();
            assert!(!valid(&bad));
            checks += 1;
            let mut bad = q.clone();
            bad.parts[i].items[0].0[0] ^= 1;
            assert!(!valid(&bad));
            checks += 1;
        }
    }
    checks
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a[1].parse().unwrap();
    let k: usize = a[2].parse().unwrap();
    let seed: u64 = a[3].parse().unwrap();
    let batches: usize = a[4].parse().unwrap();
    let size: usize = a[5].parse().unwrap();
    assert!(n > 0 && k > 0 && batches >= 4 && size >= 32);
    println!("n,k,seed,work,metric,batch,position,mode,elapsed_ns,transactions,accepted,proof_bytes_mean,proof_nodes_mean,checks");
    for work in ["meta", "right", "both"] {
        let cases: Vec<Case> = ["single_prefix", "dual_prefix", "single_hash", "dual_hash"]
            .iter()
            .map(|m| prepare(n, k, seed, m, work))
            .collect();
        let checks: Vec<usize> = cases.iter().map(correctness).collect();
        for metric in ["proof", "lookup"] {
            for c in &cases {
                for j in 0..256 {
                    let q = &c.queries[j % 32];
                    assert!(if metric == "proof" {
                        valid(q)
                    } else {
                        read_valid(q, &c.trees)
                    });
                }
            }
            for batch in 0..batches {
                // Balanced rotation with reversed cycles: each mode occupies each position.
                let mut order = vec![0, 1, 2, 3];
                order.rotate_left((batch + (seed as usize % 4)) % 4);
                if (batch / 4) % 2 == 1 {
                    order.reverse();
                }
                for (pos, &ci) in order.iter().enumerate() {
                    let c = &cases[ci];
                    let mut accepted = 0;
                    let start = Instant::now();
                    for j in 0..size {
                        let q = black_box(&c.queries[(j + batch) % 32]);
                        accepted += usize::from(black_box(if metric == "proof" {
                            valid(q)
                        } else {
                            read_valid(q, &c.trees)
                        }));
                    }
                    let elapsed = start.elapsed().as_nanos();
                    assert_eq!(accepted, size);
                    let bytes: usize = c
                        .queries
                        .iter()
                        .flat_map(|q| &q.parts)
                        .flat_map(|p| &p.proof)
                        .map(|v| v.len())
                        .sum();
                    let nodes: usize = c
                        .queries
                        .iter()
                        .flat_map(|q| &q.parts)
                        .map(|p| p.proof.len())
                        .sum();
                    println!("{n},{k},{seed},{work},{metric},{batch},{pos},{},{elapsed},{size},{accepted},{},{},{}",
       c.mode,bytes as f64/32.0,nodes as f64/32.0,checks[ci]);
                }
            }
        }
    }
}
