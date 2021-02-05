use std::{
    collections::VecDeque,
    error::Error,
    task::{Context, Poll},
    io,
    iter,
    str,
};
use libp2p::{
    PeerId,
    Multiaddr,
    swarm::{
        NetworkBehaviour,
        NegotiatedSubstream,
        ProtocolsHandler,
        SubstreamProtocol,
        NetworkBehaviourAction,
        PollParameters,
        ProtocolsHandlerUpgrErr,
        KeepAlive,
        ProtocolsHandlerEvent,
    },
    InboundUpgrade,
    OutboundUpgrade,
    core::{UpgradeInfo, connection::ConnectionId, upgrade::ReadOneError},
    identity,
    Swarm,
};
use futures::future::BoxFuture;
use void::Void;
use futures::prelude::*;
use async_std::task;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    // create a random peerid.
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    log::info!("Local peer id: {:?}", peer_id);

    // create a transport.
    let transport = libp2p::build_development_transport(id_keys)?;

    // create a echo network behaviour.
    let behaviour = EchoBehaviour::new();

    // create a swarm that establishes connections through the given transport
    // and applies the echo behaviour on each connection.
    let mut swarm = Swarm::new(transport, behaviour, peer_id);

    // Dial the peer identified by the multi-address given as the second
    // cli arg.
    if let Some(addr) = std::env::args().nth(1) {
        let remote = addr.parse()?;
        Swarm::dial_addr(&mut swarm, remote)?;
        log::info!("Dialed {}", addr)
    }

    // Tell the swarm to listen on all interfaces and a random, OS-assigned port.
    Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse()?)?;

    let mut listening = false;
    task::block_on(future::poll_fn(move |cx: &mut Context<'_>| {
        loop {
            match swarm.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => log::info!("Get event: {:?}", event),
                Poll::Ready(None) => {
                    log::info!("Swam poll next ready none");
                    return Poll::Ready(())
                },
                Poll::Pending => {
                    if !listening {
                        for addr in Swarm::listeners(&swarm) {
                            log::info!("Listening on {}", addr);
                            listening = true;
                        }
                    }
                    return Poll::Pending
                }
            }
        }
    }));

    Ok(())
}


// echo libp2p protocl implementation

pub struct EchoBehaviour {
    events: VecDeque<EchoBehaviourEvent>,
}

impl EchoBehaviour {
    pub fn new() -> Self {
        EchoBehaviour {
            events: VecDeque::new(),
        }
    }
}

#[derive(Debug)]
pub struct EchoBehaviourEvent {
    pub peer: PeerId,
    pub result: EchoHandlerEvent,
}

impl NetworkBehaviour for EchoBehaviour {
    type ProtocolsHandler = EchoHandler;
    type OutEvent = EchoBehaviourEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        EchoHandler::new()
    }

    fn addresses_of_peer(&mut self, _peer_id: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, _: &PeerId) {
        log::info!("NetworkBehaviour::inject_connected");
    }

    fn inject_disconnected(&mut self, _: &PeerId) {
        log::info!("NetworkBehaviour::inject_disconnected");
    }

    fn inject_event(&mut self, peer: PeerId, _: ConnectionId, result: EchoHandlerEvent) {
        log::info!("NetworkBehaviour::inject_event");
        self.events.push_front(EchoBehaviourEvent { peer, result })
    }

    fn poll(&mut self, _: &mut Context<'_>, _: &mut impl PollParameters)
        -> Poll<NetworkBehaviourAction<Void, EchoBehaviourEvent>>
    {
        log::info!("NetworkBehaviour::poll, events: {:?}", self.events);
        if let Some(e) = self.events.pop_back() {
            Poll::Ready(NetworkBehaviourAction::GenerateEvent(e))
        } else {
            Poll::Pending
        }
    }
}

pub struct EchoHandler {
    inbound: Option<EchoFuture>,
    outbound: Option<SendEchoFuture>,
    already_echo: bool,
}

type EchoFuture = BoxFuture<'static, Result<NegotiatedSubstream, io::Error>>;
type SendEchoFuture = BoxFuture<'static, Result<NegotiatedSubstream, io::Error>>;

#[derive(Debug)]
pub enum EchoHandlerEvent {
    Success(u8),
}

impl EchoHandler {
    pub fn new() -> Self {
        EchoHandler {
            inbound: None,
            outbound: None,
            already_echo: false,
        }
    }
}

impl ProtocolsHandler for EchoHandler {
    type InEvent = Void;
    type OutEvent = EchoHandlerEvent;
    type Error = ReadOneError;
    type InboundProtocol = EchoProtocol;
    type OutboundProtocol = EchoProtocol;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<EchoProtocol, ()> {
        SubstreamProtocol::new(EchoProtocol, ())
    }

    fn inject_fully_negotiated_inbound(&mut self, stream: NegotiatedSubstream, (): ()) {
        if self.inbound.is_some() {
            panic!("already have inbound");
        }
        log::info!("ProtocolsHandler::inject_fully_negotiated_inbound");
        self.inbound = Some(recv_echo(stream).boxed());
    }

    fn inject_fully_negotiated_outbound(&mut self, stream: NegotiatedSubstream, (): ()) {
        if self.outbound.is_some() {
            panic!("already have outbound");
        }
        log::info!("ProtocolsHandler::inject_fully_negotiated_outbound");
        self.outbound = Some(send_echo(stream).boxed());
    }

    fn inject_event(&mut self, _: Void) {
    }

    fn inject_dial_upgrade_error(&mut self, _info: (), error: ProtocolsHandlerUpgrErr<Void>) {
        log::info!("ProtocolsHandler::inject_dial_upgrade_error: {:?}", error);
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        KeepAlive::Yes
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<
        ProtocolsHandlerEvent<
            EchoProtocol,
            (),
            EchoHandlerEvent,
            Self::Error
        >
    > {
        log::info!("ProtocolsHandler::poll begins...");

        if let Some(fut) = self.inbound.as_mut() {
            match fut.poll_unpin(cx) {
                Poll::Pending => {
                    log::info!("ProtocolsHandler::poll, inbound is some but pending...");
                }
                Poll::Ready(Err(e)) => {
                    log::info!("ProtocolsHandler::poll, inbound is some but resolve with error: {:?}", e);
                    self.inbound = None;
                    panic!();
                }
                Poll::Ready(Ok(stream)) => {
                    log::info!("ProtocolsHandler::poll, inbound is some and ready with success");
                    self.inbound = Some(recv_echo(stream).boxed());
                    return Poll::Ready(ProtocolsHandlerEvent::Custom(EchoHandlerEvent::Success(1)))
                }
            }
        }

        if !self.already_echo {
            self.already_echo = true;
            let protocol = SubstreamProtocol::new(EchoProtocol, ());
            return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol
            })
        }

        match self.outbound.take() {
            Some(mut send_echo_future) => {
                match send_echo_future.poll_unpin(cx) {
                    Poll::Pending => {
                        // mxinden: The future has not yet finished. Make sure
                        // to poll it again on the next iteration.
                        self.outbound = Some(send_echo_future);
                        log::info!("ProtocolsHandler::poll, outbound is some but pending...");
                    },
                    Poll::Ready(Ok(stream)) => {
                        log::info!("ProtocolsHandler::poll, outbound is some and ready with success");
                        self.outbound = Some(send_echo(stream).boxed());
                        return Poll::Ready(
                            ProtocolsHandlerEvent::Custom(
                                EchoHandlerEvent::Success(1)
                            )
                        )
                    },
                    Poll::Ready(Err(e)) => {
                        log::info!("ProtocolsHandler::poll, outbound is some but resolve with error: {:?}", e);
                        panic!();
                    }
                }
            },
            None => {
                // mxinden: Don't use thread::sleep in a futures context. This
                // will block the entire thread, allowing no other future,
                // potentially running on that same thread, to make progress.
                // Use `futures-timer` instead. Happy to go into more details.
                // Let me know.
                //
                // thread::sleep(time::Duration::from_secs(3));

                log::info!("ProtocolsHandler::poll, outbound is none, waiting for outbound substream to be negotiated");
            },
        }
        
        Poll::Pending
    }

}

const ECHO_SIZE: usize = 12;

pub async fn send_echo<S>(mut stream: S) -> io::Result<S>
where
    S: AsyncRead + AsyncWrite + Unpin
{
    // mxinden: A bit of a hack. Likely nicer to do somewhere else.
    futures_timer::Delay::new(std::time::Duration::from_secs(3)).await;

    let payload = "hello world!";
    log::info!("send_echo, preparing send payload {:?}", payload);
    log::info!("send_echo, payload content {:?}", payload.as_bytes());
    stream.write_all(payload.as_bytes()).await?;
    stream.flush().await?;
    let mut recv_payload = [0u8; ECHO_SIZE];
    log::info!("send_echo, awaiting echo for {:?}", payload);
    
    stream.read_exact(&mut recv_payload).await?;
    log::info!("send_echo, received echo: {:?}", str::from_utf8(&recv_payload));
    if str::from_utf8(&recv_payload) == Ok(payload) {
        Ok(stream)
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, "Echo payload mismatch"))
    }
}

pub async fn recv_echo<S>(mut stream: S) -> io::Result<S>
where
    S: AsyncRead + AsyncWrite + Unpin
{
    let mut payload = [0u8; ECHO_SIZE];
    log::info!("recv_echo, waiting for echo ...");
    stream.read_exact(&mut payload).await?;
    log::info!("recv_echo, echo for {:?}", payload);
    stream.write_all(&payload).await?;
    stream.flush().await?;
    log::info!("recv_echo, receive echo successfully, {:?}", payload);
    Ok(stream)
}


#[derive(Default, Debug, Copy, Clone)]
pub struct EchoProtocol;

impl InboundUpgrade<NegotiatedSubstream> for EchoProtocol {
    type Output = NegotiatedSubstream;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, stream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        log::info!("InboundUpgrade::upgrade_inbound");
        future::ok(stream)
    }
}

impl OutboundUpgrade<NegotiatedSubstream> for EchoProtocol {
    type Output = NegotiatedSubstream;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, stream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        log::info!("OutboundUpgrade::upgrade_outbound");
        future::ok(stream)
    }
}

impl UpgradeInfo for EchoProtocol {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/ipfs/echo/1.0.0")
    }
}
