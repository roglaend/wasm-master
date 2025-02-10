use anyhow::Context;
use std::convert::TryInto;
use wit_bindgen_wrpc::{bytes::Bytes, wrpc_transport};
use wrpc_transport::ResourceBorrow;

/// Converts a resource handle into a `usize` index.
///
/// The resource handle is expected to be the little‑endian encoding of a `usize`.
pub fn handle_to_index<T>(handle: ResourceBorrow<T>) -> anyhow::Result<usize> {
    let handle_bytes = Bytes::from(handle);
    let arr: [u8; std::mem::size_of::<usize>()] =
        handle_bytes.as_ref().try_into().context("invalid handle")?;
    Ok(usize::from_le_bytes(arr))
}
