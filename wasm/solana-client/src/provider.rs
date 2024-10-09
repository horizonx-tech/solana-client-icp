use candid::Nat;
use http::StatusCode;
use ic_cdk::api::management_canister::http_request::{
    http_request, CanisterHttpRequestArgument, HttpHeader, HttpMethod,
};
use serde::de::DeserializeOwned;

use crate::{methods::Method, ClientError, ClientRequest, ClientResponse, ClientResult};

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
    ) -> ClientResult<ClientResponse<R>> {
        let client_request = ClientRequest::new(T::NAME).id(0).params(request);
        let args: CanisterHttpRequestArgument = CanisterHttpRequestArgument {
            body: Some(serde_json::to_vec(&client_request).map_err(|e| ClientError::new(e))?),
            url: self.url.clone(),
            headers: vec![HttpHeader {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            }],
            max_response_bytes: None,
            method: HttpMethod::POST,
            transform: None, //TODO
        };
        let cycles = 2_603_253_600; //TODO
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
