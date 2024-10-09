use ic_cdk::api::management_canister::http_request::{
    http_request, CanisterHttpRequestArgument, HttpHeader, HttpMethod, TransformContext,
};
use serde::de::DeserializeOwned;

use crate::{methods::Method, ClientError, ClientRequest, ClientResponse, ClientResult};

#[derive(Clone)]
pub struct CallOptions {
    max_response_bytes: Option<u64>,
    transform: Option<TransformContext>,
}

impl Default for CallOptions {
    fn default() -> Self {
        Self {
            max_response_bytes: None,
            transform: None,
        }
    }
}

// Calcurate cycles for http_request
// NOTE:
//   v0.11: https://github.com/dfinity/cdk-rs/blob/0b14facb80e161de79264c8f88b1a0c8e18ffcb6/examples/management_canister/src/caller/lib.rs#L7-L19
//   v0.8: https://github.com/dfinity/cdk-rs/blob/a8454cb37420c200c7b224befd6f68326a01442e/src/ic-cdk/src/api/management_canister/http_request.rs#L290-L299
fn http_request_required_cycles(arg: &CanisterHttpRequestArgument) -> u128 {
    let max_response_bytes = match arg.max_response_bytes {
        Some(ref n) => *n as u128,
        None => 2 * 1024 * 1024u128, // default 2MiB
    };
    let arg_raw = candid::utils::encode_args((arg,)).expect("Failed to encode arguments.");
    // The fee is for a 13-node subnet to demonstrate a typical usage.
    (3_000_000u128
        + 60_000u128 * 13
        + (arg_raw.len() as u128 + "http_request".len() as u128) * 400
        + max_response_bytes * 800)
        * 13
}

#[derive(Clone)]
pub struct HttpProvider {
    url: String,
    timeout: u32,
}

impl HttpProvider {
    pub fn new(url: impl ToString) -> Self {
        Self {
            url: url.to_string(),
            timeout: 60000,
        }
    }

    pub fn new_with_timeout(url: impl ToString, timeout: u32) -> Self {
        Self {
            url: url.to_string(),
            timeout,
        }
    }
}

impl HttpProvider {
    pub async fn send<T: Method, R: DeserializeOwned>(
        &self,
        request: &T,
        opts: CallOptions,
    ) -> ClientResult<ClientResponse<R>> {
        let client_request = ClientRequest::new(T::NAME).id(0).params(request);
        let args: CanisterHttpRequestArgument = CanisterHttpRequestArgument {
            body: Some(serde_json::to_vec(&client_request).map_err(|e| ClientError::new(e))?),
            url: self.url.clone(),
            headers: vec![HttpHeader {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            }],
            max_response_bytes: opts.max_response_bytes,
            method: HttpMethod::POST,
            transform: opts.transform, //TODO
        };
        let cycles = http_request_required_cycles(&args);
        match http_request(args, cycles).await {
            Ok(response) => {
                let status = response.0.status;
                if status.eq(&200_u8) {
                    let response =
                        serde_json::from_slice(&response.0.body).map_err(ClientError::new)?;
                    return Ok(response);
                } else {
                    let body: serde_json::Value =
                        serde_json::from_slice(&response.0.body).map_err(ClientError::new)?;
                    Err(ClientError::new(format!(
                        "HTTP error: {}",
                        body.to_string()
                    )))
                }
            }
            Err(e) => Err(ClientError::new(e.1)),
        }
    }
}

#[derive(Clone)]
pub enum Provider {
    Http(HttpProvider),
}

impl Provider {
    pub fn new(url: &str) -> Self {
        Self::Http(HttpProvider::new(url))
    }
}
