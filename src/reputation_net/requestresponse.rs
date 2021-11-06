use serde::{Deserialize, Serialize};

/// A number of responses can be sent in response to gossipsub requests, so technically they are sent as requests.
/// It's easiest to just keep both in one type.
#[derive(Debug,Serialize,Deserialize)]
pub enum RpcRequestResponse {
    None,
    Hello(String),
}