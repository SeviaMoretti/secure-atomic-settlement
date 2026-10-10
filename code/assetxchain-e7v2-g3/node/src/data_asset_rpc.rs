use std::sync::Arc;
use jsonrpsee::{
    core::RpcResult,
    proc_macros::rpc,
    types::error::ErrorObject,
};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sc_client_api::ProofProvider;
use sp_core::storage::ChildInfo;
use sp_runtime::traits::Block as BlockT;

/// 定义暴露给轻客户端的 RPC 接口
#[rpc(client, server)]
pub trait DataAssetApi<BlockHash> {
    #[method(name = "dataAssets_getAssetProof")]
    fn get_asset_proof(&self, asset_id: [u8; 32], at: Option<BlockHash>) -> RpcResult<Option<Vec<Vec<u8>>>>;
}

pub struct DataAssetRpcImpl<C, B> {
    client: Arc<C>,
    _marker: std::marker::PhantomData<B>,
}

impl<C, B> DataAssetRpcImpl<C, B> {
    pub fn new(client: Arc<C>) -> Self {
        Self { client, _marker: Default::default() }
    }
}

// 实现 RPC 接口
impl<C, Block> DataAssetApiServer<<Block as BlockT>::Hash> for DataAssetRpcImpl<C, Block>
where
    Block: BlockT,
    // 需要ProofProvider让节点能生成底层存储树的默克尔证明
    C: Send + Sync + 'static + ProvideRuntimeApi<Block> + HeaderBackend<Block> + ProofProvider<Block>,
{
    fn get_asset_proof(&self, asset_id: [u8; 32], at: Option<<Block as BlockT>::Hash>) -> RpcResult<Option<Vec<Vec<u8>>>> {
        let api = self.client.clone();
        let hash = at.unwrap_or_else(|| api.info().best_hash);

        // 与pallet中定义的子树ID一致：asset_trie
        let child_info = ChildInfo::new_default(b":asset_trie:");
        
        // 构造资产在子树中的键名："assets/" + asset_id
        let mut key = b"assets/".to_vec();
        key.extend_from_slice(&asset_id);

        // 调用Substrate原生的客户端API生成子树的读证明
        let proof = api.read_child_proof(
            hash,
            &child_info,
            &mut std::iter::once(key.as_slice()),
        ).map_err(|e| ErrorObject::owned(
            1,
            format!("Failed to generate proof: {:?}", e),
            None::<()>,
        ))?;

        // Trie树节点数据（Merkle Proof路径）
        Ok(Some(proof.into_iter_nodes().collect()))
    }
}