use codec::{Encode, Decode};
use sp_core::H256;
use sp_runtime::DigestItem;
use alloc::vec::Vec; 

const ASSET_ROOT_PREFIX: &[u8] = b"ASSET_ROOT";// asset_trie
const CERTIFICATE_ROOT_PREFIX: &[u8] = b"CERT_ROOT";// certificate_trie

pub fn create_asset_root_digest(root: H256) -> DigestItem {
    let mut data = Vec::new();
    data.extend_from_slice(ASSET_ROOT_PREFIX);
    data.extend_from_slice(&root.encode());
    DigestItem::Other(data)
}

pub fn create_certificate_root_digest(root: H256) -> DigestItem {
    let mut data = Vec::new();
    data.extend_from_slice(CERTIFICATE_ROOT_PREFIX);
    data.extend_from_slice(&root.encode());
    DigestItem::Other(data)
}

fn extract_root_by_prefix(digest: &sp_runtime::Digest, prefix: &[u8]) -> Option<H256> {
    for log in digest.logs.iter() {
        if let DigestItem::Other(data) = log {
            if data.len() > prefix.len() && &data[..prefix.len()] == prefix {
                if let Ok(root) = H256::decode(&mut &data[prefix.len()..]) {
                    return Some(root);
                }
            }
        }
    }
    None
}

pub fn extract_asset_root(digest: &sp_runtime::Digest) -> Option<H256> {
    extract_root_by_prefix(digest, ASSET_ROOT_PREFIX)
}

pub fn extract_certificate_root(digest: &sp_runtime::Digest) -> Option<H256> {
    extract_root_by_prefix(digest, CERTIFICATE_ROOT_PREFIX)
}