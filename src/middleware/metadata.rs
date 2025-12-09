use alloy::primitives::Address;
use axum::http::Extensions;
use rpc::Request as JsonRpcRequest;

/// Common request metadata shared across middleware layers.
#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub json_rpc: Option<JsonRpcRequest>,
    pub size: Option<usize>,
    pub signer: Option<Address>,
    pub trace_id: Option<String>,
}

/// Ensure metadata exists in extensions and apply an update function.
pub fn with_metadata<F>(extensions: &mut Extensions, f: F)
where
    F: FnOnce(&mut Metadata),
{
    if extensions.get::<Metadata>().is_none() {
        extensions.insert(Metadata::default());
    }

    // Safe because we inserted above if it was absent.
    let metadata = extensions
        .get_mut::<Metadata>()
        .expect("metadata should be present");
    f(metadata);
}

