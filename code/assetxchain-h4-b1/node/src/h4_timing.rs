//! Opt-in H4 host-side Runtime-call timing. Never changes Runtime state.
use sp_core::traits::{CallContext,CodeExecutor,Externalities,ReadRuntimeVersion,RuntimeCode};
use sc_executor::{WasmExecutor,RuntimeVersionOf};
use std::sync::{Arc,atomic::{AtomicU64,Ordering}};
use std::time::Instant;
type Inner=WasmExecutor<sp_io::SubstrateHostFunctions>;
#[derive(Clone)]
pub struct TimedExecutor { inner:Inner, enabled:bool, epoch:Arc<Instant>, sequence:Arc<AtomicU64> }
fn key(item:&[u8])->Vec<u8>{[sp_core::twox_128(b"System").as_slice(),sp_core::twox_128(item).as_slice()].concat()}
fn number(ext:&mut dyn Externalities,item:&[u8])->Option<u32>{ext.storage(&key(item)).and_then(|x|x.get(..4).map(|v|u32::from_le_bytes(v.try_into().unwrap())))}
fn optional(n:Option<u32>)->String{n.map(|x|x.to_string()).unwrap_or_else(||"null".into())}
impl TimedExecutor {
 pub fn new(inner:Inner)->Self{Self{inner,enabled:std::env::var("H4_EXECUTION_TIMING").as_deref()==Ok("1"),epoch:Arc::new(Instant::now()),sequence:Arc::new(AtomicU64::new(0))}}
}
impl ReadRuntimeVersion for TimedExecutor {
 fn read_runtime_version(&self,wasm:&[u8],ext:&mut dyn Externalities)->Result<Vec<u8>,String>{self.inner.read_runtime_version(wasm,ext)}
}
impl RuntimeVersionOf for TimedExecutor {
 fn runtime_version(&self,ext:&mut dyn Externalities,code:&RuntimeCode)->sc_executor::error::Result<sp_version::RuntimeVersion>{self.inner.runtime_version(ext,code)}
}
impl CodeExecutor for TimedExecutor {
 type Error=<Inner as CodeExecutor>::Error;
 fn call(&self,ext:&mut dyn Externalities,code:&RuntimeCode,method:&str,data:&[u8],context:CallContext)->(Result<Vec<u8>,Self::Error>,bool){
  if !self.enabled || !matches!(method,"BlockBuilder_apply_extrinsic"|"Core_execute_block"|"TaggedTransactionQueue_validate_transaction") {return self.inner.call(ext,code,method,data,context)}
  let seq=self.sequence.fetch_add(1,Ordering::Relaxed);
  let block=number(ext,b"Number");let index=ext.storage(b":extrinsic_index").and_then(|x|x.get(..4).map(|v|u32::from_le_bytes(v.try_into().unwrap())));
  let parent=ext.storage(&key(b"ParentHash")).map(|b|format!("0x{}",sp_core::hexdisplay::HexDisplay::from(&b))).unwrap_or_default();
  let start=Instant::now();let start_ns=start.duration_since(*self.epoch).as_nanos();
  let result=self.inner.call(ext,code,method,data,context);
  let end=Instant::now();let duration=end.duration_since(start).as_nanos();let end_ns=end.duration_since(*self.epoch).as_nanos();
  // Hashing, formatting and I/O are outside the recorded interval; their perturbation is calibrated.
  let hash=sp_core::blake2_256(data);
  eprintln!("H4_SPAN {{\"schema\":\"h4-host-call-v1\",\"sequence\":{},\"method\":\"{}\",\"context\":\"{:?}\",\"payload_hash\":\"0x{}\",\"block_number\":{},\"extrinsic_index\":{},\"parent_hash\":\"{}\",\"start_ns\":{},\"end_ns\":{},\"duration_ns\":{},\"host_call_ok\":{},\"native\":{}}}",seq,method,context,sp_core::hexdisplay::HexDisplay::from(&hash),optional(block),optional(index),parent,start_ns,end_ns,duration,result.0.is_ok(),result.1);
  result
 }
}
