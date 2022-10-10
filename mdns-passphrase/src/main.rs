use futures::{StreamExt, AsyncRead};
use libp2p::{
    identity,
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    swarm::{Swarm, SwarmEvent, behaviour},
    PeerId, request_response::{RequestResponse, RequestResponseCodec, ProtocolName, RequestResponseEvent}, NetworkBehaviour, core::upgrade::read_length_prefixed,
};
use std::error::Error;
use async_trait::async_trait;
use async_std::io;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    
    log::info!("Local peer id: {:?}", peer_id);

    let transport = libp2p::development_transport(id_keys).await?;

    let my_behaviour = Mdns::new(MdnsConfig::default()).await?;

    let mut swarm = Swarm::new(transport, my_behaviour, peer_id);
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::Behaviour(MdnsEvent::Discovered(peers)) => {
                for (peer, addr) in peers {
                    log::info!("discovered {} {}", peer, addr);
                }
            }
            SwarmEvent::Behaviour(MdnsEvent::Expired(expired)) => {
                for (peer, addr) in expired {
                    log::info!("expired {} {}", peer, addr);
                }
            }
            _ => {}
        }
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "MyOutEvent")]
struct MyBehaviour {
    request_response: RequestResponse<PassphraseVerifyCodec>,
    mdns: Mdns,
}

enum MyOutEvent {
    RequestResponse(RequestResponseEvent<PassphraseVerifyRequest, PassphraseVeifyResponse>),
    Mdns(MdnsEvent),
}

impl From<MdnsEvent> for MyOutEvent {
    fn from(v: MdnsEvent) -> Self {
        Self::Mdns(v)
    }
}

impl From<RequestResponseEvent<PassphraseVerifyRequest, PassphraseVeifyResponse>> for MyOutEvent {
    fn from(v: RequestResponseEvent<PassphraseVerifyRequest, PassphraseVeifyResponse>) -> Self {
        Self::RequestResponse(v)
    }
}

#[derive(Clone)]
struct PassphraseVerifyCodec();

#[derive(Clone)]
struct PassphraseVerifyProtocol();

impl ProtocolName for PassphraseVerifyProtocol {
    fn protocol_name(&self) -> &[u8] {
        "/passphrase-verify/1".as_bytes()
    }
}

struct PassphraseVerifyRequest(String);

struct PassphraseVeifyResponse(String);

#[async_trait]
impl RequestResponseCodec for PassphraseVerifyCodec {
    type Protocol = PassphraseVerifyProtocol;
    type Request = PassphraseVerifyRequest;
    type Response = PassphraseVeifyResponse;

    async fn read_request<T>(
        &mut self,
        _: &PassphraseVerifyProtocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let vec = read_length_prefixed(io, 6).await?;
        if vec.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        Ok(PassphraseVerifyRequest(String::from_utf8(vec).unwrap()))
    }

    async fn read_response<T>(
        &mut self,
        _: &PassphraseVerifyProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let vec = read_length_prefixed(io, 1024).await?;
        if vec.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        Ok(PassphraseVeifyResponse(String::from_utf8(vec).unwrap()))
    }

    async fn write_request<T>(
        
    )
}
