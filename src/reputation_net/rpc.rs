use async_std::io::{Result};
use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite};

use libp2p::request_response::*;
use libp2p::core::upgrade::{read_length_prefixed,write_length_prefixed};

use super::messages::*;

#[derive(Debug, Clone)]
pub struct RpcCodec {}

/// The RPC protocol
#[derive(Clone)]
pub enum RpcProtocol {
    Version1,
}

impl ProtocolName for RpcProtocol{
    fn protocol_name(&self) -> &[u8] {
        b"/reputation-net/1.0"
    }
}

#[async_trait]
impl RequestResponseCodec for RpcCodec {
    type Protocol = RpcProtocol;
    type Request = NetworkMessage;
    type Response = NetworkMessage;

    async fn read_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let data = read_length_prefixed(io, 1000).await?;
        let request = serde_json::from_slice(&data)?;
        Ok(request)
    }

    async fn read_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let data = read_length_prefixed(io, 20000).await?;
        let response = serde_json::from_slice(&data)?;
        Ok(response)
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let json_data = serde_json::to_vec(&req).unwrap();
        write_length_prefixed(io, &json_data).await
    }

    async fn write_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let json_data = serde_json::to_vec(&res).unwrap();
        write_length_prefixed(io, &json_data).await
    }
}