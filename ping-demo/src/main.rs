use std::{
    num::NonZeroU32,
    time::Duration,
    collections::VecDeque,
    error::Error,
    task::{Context, Poll},
    io,
    fmt,
    iter,
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
    },
    InboundUpgrade,
    OutboundUpgrade,
    core::{UpgradeInfo, connection::ConnectionId},
};
use wasm_timer::{Delay, Instant};
use futures::future::BoxFuture;
use void::Void;
use futures::prelude::*;
use rand::{distributions, prelude::*};

fn main() {
    println!("Hello, world!");
}


// Ping protocl implementation

pub struct Ping {
    config: PingConfig,
    events: VecDeque<PingEvent>,
}

#[derive(Debug)]
pub struct PingEvent {
    pub peer: PeerId,
    pub result: PingResult,
}

pub type PingResult = Result<PingSuccess, PingFailure>;

#[derive(Debug)]
pub enum PingSuccess {
    Pong,
    Ping { rtt: Duration }
}

#[derive(Debug)]
pub enum PingFailure {
    Timeout,
    Other { error: Box<dyn std::error::Error + Send + 'static> }
}

impl fmt::Display for PingFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PingFailure::Timeout => f.write_str("Ping timeout"),
            PingFailure::Other { error } => write!(f, "Ping error: {}", error)
        }
    }
}

impl Error for PingFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            PingFailure::Timeout => None,
            PingFailure::Other { error } => Some(&**error)
        }
    }
}

#[derive(Clone, Debug)]
pub struct PingConfig {
    timeout: Duration,
    interval: Duration,
    max_failures: NonZeroU32,
    keep_alive: bool,
}

impl NetworkBehaviour for Ping {
    type ProtocolsHandler = PingHandler;
    type OutEvent = PingEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        PingHandler::new(self.config.clone())
    }

    fn addresses_of_peer(&mut self, _peer_id: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, _: &PeerId) {}

    fn inject_disconnected(&mut self, _: &PeerId) {}

    fn inject_event(&mut self, peer: PeerId, _: ConnectionId, result: PingResult) {
        self.events.push_front(PingEvent { peer, result })
    }

    fn poll(&mut self, _: &mut Context<'_>, _: &mut impl PollParameters)
        -> Poll<NetworkBehaviourAction<Void, PingEvent>>
    {
        if let Some(e) = self.events.pop_back() {
            Poll::Ready(NetworkBehaviourAction::GenerateEvent(e))
        } else {
            Poll::Pending
        }
    }
}

pub struct PingHandler {
    config: PingConfig,
    timer: Delay,
    pending_errors: VecDeque<PingFailure>,
    failures: u32,
    outbound: Option<PingState>,
    inbound: Option<PongFuture>,
}

enum PingState {
    OpenStream,
    Idle(NegotiatedSubstream),
    Ping(PingFuture),
}

type PingFuture = BoxFuture<'static, Result<(NegotiatedSubstream, Duration), io::Error>>;
type PongFuture = BoxFuture<'static, Result<NegotiatedSubstream, io::Error>>;

impl ProtocolsHandler for PingHandler {
    type InEvent = Void;
    type OutEvent = PingResult;
    type Error = PingFailure;
    type InboundProtocol = PingProtocol;
    type OutboundProtocol = PingProtocol;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<PingProtocol, ()> {
        SubstreamProtocol::new(PingProtocol, ())
    }

    fn inject_fully_negotiated_inbound(&mut self, stream: NegotiatedSubstream, (): ()) {
        self.inbound = Some(recv_ping(stream).boxed());
    }

    fn inject_fully_negotiated_outbound(&mut self, stream: NegotiatedSubstream, (): ()) {
        self.timer.reset(self.config.timeout);
        self.outbound = Some(PingState::Ping(send_ping(stream).boxed()));
    }

    fn inject_event(&mut self, _: Void) {}

    fn inject_dial_upgrade_error(&mut self, _info: (), error: ProtocolsHandlerUpgrErr<Void>) {
        self.outbound = None; // Request a new substream on the next `poll`.
        self.pending_errors.push_front(
            match error {
                ProtocolsHandlerUpgrErr::Timeout => PingFailure::Timeout,
                e => PingFailure::Other { error: Box::new(e) },
            }
        )
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        if self.config.keep_alive {
            KeepAlive::Yes
        } else {
            KeepALive::No
        }
    }
}

const PING_SIZE: usize = 32;

pub async fn recv_ping<S>(mut stream: S) -> io::Result<S>
where
    S: AsyncRead + AsyncWrite + Unpin
{
    let mut payload = [0u8; PING_SIZE];
    log::debug!("Waiting for ping ...");
    stream.read_exact(&mut payload).await?;
    log::debug!("Sending pong for {:?}", payload);
    stream.write_all(&payload).await?;
    stream.flush().await?;
    Ok(stream)
}

pub async fn send_ping<S>(mut stream: S) -> io::Result<(S, Duration)>
where
    S: AsyncRead + AsyncWrite + Unpin
{
    let payload: [u8; PING_SIZE] = thread_rng().sample(distributions::Standard);
    log::debug!("Preparing ping payload {:?}", payload);
    stream.write_all(&payload).await?;
    stream.flush().await?;
    let started = Instant::now();
    let mut recv_payload = [0u8; PING_SIZE];
    log::debug!("Awaiting pong for {:?}", payload);
    stream.read_exact(&mut recv_payload).await?;
    if recv_payload == payload {
        Ok((stream, started.elapsed()))
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, "Ping payload mismatch"))
    }
}

#[derive(Default, Debug, Copy, Clone)]
pub struct PingProtocol;

impl InboundUpgrade<NegotiatedSubstream> for PingProtocol {
    type Output = NegotiatedSubstream;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, stream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        future::ok(stream)
    }
}

impl OutboundUpgrade<NegotiatedSubstream> for PingProtocol {
    type Output = NegotiatedSubstream;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, stream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        future::ok(stream)
    }
}

impl UpgradeInfo for PingProtocol {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/ipfs/ping/1.0.0")
    }
}