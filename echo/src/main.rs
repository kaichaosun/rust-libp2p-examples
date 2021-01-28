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
        ProtocolsHandlerEvent,
    },
    InboundUpgrade,
    OutboundUpgrade,
    core::{UpgradeInfo, connection::ConnectionId},
    identity,
    Swarm,
};
use wasm_timer::{Delay, Instant};
use futures::future::BoxFuture;
use void::Void;
use futures::prelude::*;
use rand::{distributions, prelude::*};
use async_std::task;

fn main() -> Result<(), Box<dyn Error>> {
    println!("Hello, world!");
    env_logger::init();

    // create a random peerid.
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    println!("Local peer id: {:?}", peer_id);

    // create a transport.
    let transport = libp2p::build_development_transport(id_keys)?;

    // create a Echo network behaviour.
    let behaviour = Echo::new(EchoConfig::new().with_keep_alive(true));

    // create a swarm that establishes connections through the given transport
    // and applies the Echo behaviour on each connection.
    let mut swarm = Swarm::new(transport, behaviour, peer_id);

    // Dial the peer identified by the multi-address given as the second
    // cli arg.
    if let Some(addr) = std::env::args().nth(1) {
        let remote = addr.parse()?;
        Swarm::dial_addr(&mut swarm, remote)?;
        println!("Dialed {}", addr)
    }

    // Tell the swarm to listen on all interfaces and a random, OS-assigned port.
    Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse()?)?;

    let mut listening = false;
    task::block_on(future::poll_fn(move |cx: &mut Context<'_>| {
        loop {
            match swarm.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => println!("{:?}", event),
                Poll::Ready(None) => return Poll::Ready(()),
                Poll::Pending => {
                    if !listening {
                        for addr in Swarm::listeners(&swarm) {
                            println!("Listening on {}", addr);
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


// Echo protocl implementation

pub struct Echo {
    config: EchoConfig,
    events: VecDeque<EchoEvent>,
}

impl Echo {
    pub fn new(config: EchoConfig) -> Self {
        Echo {
            config,
            events: VecDeque::new(),
        }
    }
}

#[derive(Debug)]
pub struct EchoEvent {
    pub peer: PeerId,
    pub result: EchoResult,
}

pub type EchoResult = Result<EchoSuccess, EchoFailure>;

#[derive(Debug)]
pub enum EchoSuccess {
    Pong,
    Echo { rtt: Duration }
}

#[derive(Debug)]
pub enum EchoFailure {
    Timeout,
    Other { error: Box<dyn std::error::Error + Send + 'static> }
}

impl fmt::Display for EchoFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EchoFailure::Timeout => f.write_str("Echo timeout"),
            EchoFailure::Other { error } => write!(f, "Echo error: {}", error)
        }
    }
}

impl Error for EchoFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            EchoFailure::Timeout => None,
            EchoFailure::Other { error } => Some(&**error)
        }
    }
}

#[derive(Clone, Debug)]
pub struct EchoConfig {
    timeout: Duration,
    interval: Duration,
    max_failures: NonZeroU32,
    keep_alive: bool,
}

impl EchoConfig {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(20),
            interval: Duration::from_secs(15),
            max_failures: NonZeroU32::new(1).expect("1 != 0"),
            keep_alive: false,
        }
    }

    pub fn with_keep_alive(mut self, b: bool) -> Self {
        self.keep_alive = b;
        self
    }
}

impl NetworkBehaviour for Echo {
    type ProtocolsHandler = EchoHandler;
    type OutEvent = EchoEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        EchoHandler::new(self.config.clone())
    }

    fn addresses_of_peer(&mut self, _peer_id: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, _: &PeerId) {}

    fn inject_disconnected(&mut self, _: &PeerId) {}

    fn inject_event(&mut self, peer: PeerId, _: ConnectionId, result: EchoResult) {
        self.events.push_front(EchoEvent { peer, result })
    }

    fn poll(&mut self, _: &mut Context<'_>, _: &mut impl PollParameters)
        -> Poll<NetworkBehaviourAction<Void, EchoEvent>>
    {
        if let Some(e) = self.events.pop_back() {
            Poll::Ready(NetworkBehaviourAction::GenerateEvent(e))
        } else {
            Poll::Pending
        }
    }
}

pub struct EchoHandler {
    config: EchoConfig,
    timer: Delay,
    pending_errors: VecDeque<EchoFailure>,
    failures: u32,
    outbound: Option<EchoState>,
    inbound: Option<PongFuture>,
}

impl EchoHandler {
    /// Builds a new `EchoHandler` with the given configuration.
    pub fn new(config: EchoConfig) -> Self {
        EchoHandler {
            config,
            timer: Delay::new(Duration::new(0, 0)),
            pending_errors: VecDeque::with_capacity(2),
            failures: 0,
            outbound: None,
            inbound: None,
        }
    }
}

enum EchoState {
    OpenStream,
    Idle(NegotiatedSubstream),
    Echo(EchoFuture),
}

type EchoFuture = BoxFuture<'static, Result<(NegotiatedSubstream, Duration), io::Error>>;
type PongFuture = BoxFuture<'static, Result<NegotiatedSubstream, io::Error>>;

impl ProtocolsHandler for EchoHandler {
    type InEvent = Void;
    type OutEvent = EchoResult;
    type Error = EchoFailure;
    type InboundProtocol = EchoProtocol;
    type OutboundProtocol = EchoProtocol;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<EchoProtocol, ()> {
        SubstreamProtocol::new(EchoProtocol, ())
    }

    fn inject_fully_negotiated_inbound(&mut self, stream: NegotiatedSubstream, (): ()) {
        self.inbound = Some(recv_echo(stream).boxed());
    }

    fn inject_fully_negotiated_outbound(&mut self, stream: NegotiatedSubstream, (): ()) {
        self.timer.reset(self.config.timeout);
        self.outbound = Some(EchoState::Echo(send_echo(stream).boxed()));
    }

    fn inject_event(&mut self, _: Void) {}

    fn inject_dial_upgrade_error(&mut self, _info: (), error: ProtocolsHandlerUpgrErr<Void>) {
        self.outbound = None; // Request a new substream on the next `poll`.
        self.pending_errors.push_front(
            match error {
                ProtocolsHandlerUpgrErr::Timeout => EchoFailure::Timeout,
                e => EchoFailure::Other { error: Box::new(e) },
            }
        )
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        if self.config.keep_alive {
            KeepAlive::Yes
        } else {
            KeepAlive::No
        }
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<ProtocolsHandlerEvent<EchoProtocol, (), EchoResult, Self::Error>> {
        // respond to inbound Echos.
        if let Some(fut) = self.inbound.as_mut() {
            match fut.poll_unpin(cx) {
                Poll::Pending => {}
                Poll::Ready(Err(e)) => {
                    log::debug!("Inbound Echo error: {:?}", e);
                    self.inbound = None;
                }
                Poll::Ready(Ok(stream)) => {
                    self.inbound = Some(recv_echo(stream).boxed());
                    return Poll::Ready(ProtocolsHandlerEvent::Custom(Ok(EchoSuccess::Pong)))
                }
            }
        }

        loop {
            // check for outbound Echo failures.
            if let Some(error) = self.pending_errors.pop_back() {
                log::debug!("Echo failure: {:?}", error);

                self.failures += 1;

                if self.failures > 1 || self.config.max_failures.get() > 1 {
                    if self.failures >= self.config.max_failures.get() {
                        log::debug!("Too many failures ({}). Closing connection.", self.failures);
                        return Poll::Ready(ProtocolsHandlerEvent::Close(error))
                    }

                    return Poll::Ready(ProtocolsHandlerEvent::Custom(Err(error)))
                }
            }

            // continue outbound Echos
            match self.outbound.take() {
                Some(EchoState::Echo(mut echo)) => match echo.poll_unpin(cx) {
                    Poll::Pending => {
                        if self.timer.poll_unpin(cx).is_ready() {
                            self.pending_errors.push_front(EchoFailure::Timeout);
                        } else {
                            self.outbound = Some(EchoState::Echo(echo));
                            break
                        }
                    },
                    Poll::Ready(Ok((stream, rtt))) => {
                        self.failures = 0;
                        self.timer.reset(self.config.interval);
                        self.outbound = Some(EchoState::Idle(stream));
                        return Poll::Ready(
                            ProtocolsHandlerEvent::Custom(
                                Ok(EchoSuccess::Echo { rtt })
                            )
                        )
                    },
                    Poll::Ready(Err(e)) => {
                        self.pending_errors.push_front(EchoFailure::Other {
                            error: Box::new(e)
                        })
                    }
                },
                Some(EchoState::Idle(stream)) => match self.timer.poll_unpin(cx) {
                    Poll::Pending => {
                        self.outbound = Some(EchoState::Idle(stream));
                        break
                    },
                    Poll::Ready(Ok(())) => {
                        self.timer.reset(self.config.timeout);
                        self.outbound = Some(EchoState::Echo(send_echo(stream).boxed()));
                    },
                    Poll::Ready(Err(e)) => {
                        return Poll::Ready(
                            ProtocolsHandlerEvent::Close(
                                EchoFailure::Other { error: Box::new(e) } 
                            )
                        )
                    }
                },
                Some(EchoState::OpenStream) => {
                    self.outbound = Some(EchoState::OpenStream);
                    break
                },
                None => {
                    self.outbound = Some(EchoState::OpenStream);
                    let protocol = SubstreamProtocol::new(EchoProtocol, ())
                        .with_timeout(self.config.timeout);
                    return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        protocol
                    })
                }
            }
        }

        Poll::Pending
    }

}

const ECHO_SIZE: usize = 32;

pub async fn recv_echo<S>(mut stream: S) -> io::Result<S>
where
    S: AsyncRead + AsyncWrite + Unpin
{
    let mut payload = [0u8; ECHO_SIZE];
    log::debug!("Waiting for Echo ...");
    stream.read_exact(&mut payload).await?;
    log::debug!("Sending pong for {:?}", payload);
    stream.write_all(&payload).await?;
    stream.flush().await?;
    Ok(stream)
}

pub async fn send_echo<S>(mut stream: S) -> io::Result<(S, Duration)>
where
    S: AsyncRead + AsyncWrite + Unpin
{
    let payload: [u8; ECHO_SIZE] = thread_rng().sample(distributions::Standard);
    log::debug!("Preparing Echo payload {:?}", payload);
    stream.write_all(&payload).await?;
    stream.flush().await?;
    let started = Instant::now();
    let mut recv_payload = [0u8; ECHO_SIZE];
    log::debug!("Awaiting pong for {:?}", payload);
    
    stream.read_exact(&mut recv_payload).await?;
    if recv_payload == payload {
        Ok((stream, started.elapsed()))
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, "Echo payload mismatch"))
    }
}

#[derive(Default, Debug, Copy, Clone)]
pub struct EchoProtocol;

impl InboundUpgrade<NegotiatedSubstream> for EchoProtocol {
    type Output = NegotiatedSubstream;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, stream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        future::ok(stream)
    }
}

impl OutboundUpgrade<NegotiatedSubstream> for EchoProtocol {
    type Output = NegotiatedSubstream;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, stream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        future::ok(stream)
    }
}

impl UpgradeInfo for EchoProtocol {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/ipfs/Echo/1.0.0")
    }
}