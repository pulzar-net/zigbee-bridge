//! An async EZSP client over an ASH link.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};
use zigbee_ash::{Config as AshConfig, Event as AshEvent, Link, LinkError, LinkEvent, Role};
use zigbee_types::wire::Reader;

use crate::callback::Callback;
use crate::command::{Command, DecodeError, Response};
use crate::frame::{self, Format, HeaderError};
use crate::ids;
use crate::types::{
    ProtocolVersion, StackVersion, Status, MAX_PROTOCOL_VERSION, MIN_PROTOCOL_VERSION,
};

/// What the NCP said about itself during start-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NcpInfo {
    /// Negotiated protocol version.
    pub protocol: ProtocolVersion,
    /// Stack type (2 for a Zigbee mesh stack).
    pub stack_type: u8,
    /// The `version` command's packed stack version (one nibble per part).
    pub stack_version_packed: u16,
    /// The full EmberZNet version from `VERSION_INFO`.
    pub stack_version: StackVersion,
    /// Why the NCP last reset (the RSTACK code).
    pub reset_code: u8,
}

/// Unpack the `version` command's stack version: `0x7440` is 7.4.4.0.
pub fn unpack_stack_version(packed: u16) -> (u8, u8, u8, u8) {
    (
        (packed >> 12) as u8,
        ((packed >> 8) & 0xF) as u8,
        ((packed >> 4) & 0xF) as u8,
        (packed & 0xF) as u8,
    )
}

/// Client errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EzspError {
    /// The ASH link failed.
    #[error("ASH link: {0}")]
    Link(#[from] LinkError),
    /// The link went down (NCP reset, timeouts, port closed).
    #[error("NCP link down: {0}")]
    Down(String),
    /// No RSTACK: nothing that speaks ASH is on the port.
    #[error("no ASH reply (is this an EZSP adapter at the right baud rate?)")]
    NoReply,
    /// No response to a command in time.
    #[error("no response to frame {0:#06x}")]
    Timeout(u16),
    /// A frame header did not parse.
    #[error("bad EZSP header: {0}")]
    Header(#[from] HeaderError),
    /// A response did not parse.
    #[error("bad EZSP response: {0}")]
    Decode(#[from] DecodeError),
    /// The NCP rejected the command as invalid.
    #[error("NCP rejected frame {frame_id:#06x} as invalid (reason {reason:#04x})")]
    InvalidCommand {
        /// The command.
        frame_id: u16,
        /// `EzspStatus` reason.
        reason: u8,
    },
    /// The command ran and returned a failure status.
    #[error("frame {frame_id:#06x} failed: {status}")]
    Status {
        /// The command.
        frame_id: u16,
        /// The status.
        status: Status,
    },
    /// The NCP answered with a response that does not fit the command.
    #[error("unexpected response {0:?}")]
    Unexpected(Box<Response>),
    /// The NCP's protocol version is outside what this crate speaks.
    #[error(
        "EZSP protocol {protocol} (EmberZNet {}.{}.{}) is not supported: need {MIN_PROTOCOL_VERSION} to {MAX_PROTOCOL_VERSION} (EmberZNet 7.4 or newer)",
        unpack_stack_version(*stack_version).0, unpack_stack_version(*stack_version).1, unpack_stack_version(*stack_version).2
    )]
    Unsupported {
        /// The NCP's protocol version.
        protocol: u8,
        /// Its packed stack version.
        stack_version: u16,
    },
    /// The NCP is not a Zigbee stack.
    #[error("stack type {0} is not a Zigbee mesh stack")]
    StackType(u8),
}

/// Something the NCP sent that is not a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EzspEvent {
    /// A callback.
    Callback(Callback),
    /// The link is down; every later call fails. Start a new client.
    Down(String),
}

/// Client settings.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// The ASH link's settings.
    pub ash: AshConfig,
    /// How long to wait for each response.
    pub response_timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            ash: AshConfig::default(),
            response_timeout: Duration::from_secs(12),
        }
    }
}

struct Pending {
    seq: u8,
    frame_id: u16,
    reply: oneshot::Sender<Result<Response, EzspError>>,
}

struct Inner {
    link: Link,
    version: AtomicU8,
    extended: AtomicBool,
    down: Mutex<Option<String>>,
    seq: tokio::sync::Mutex<u8>,
    pending: Mutex<Option<Pending>>,
    timeout: Duration,
}

/// An EZSP client. Clones share one connection; commands are sent one at a
/// time, as EZSP requires.
#[derive(Clone)]
pub struct Ezsp {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Ezsp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ezsp")
            .field("version", &self.version())
            .finish_non_exhaustive()
    }
}

/// Reset the link (RST) and wait for RSTACK. Returns the reset code.
async fn connect_link(
    link: &Link,
    events: &mut mpsc::Receiver<LinkEvent>,
    config: &AshConfig,
) -> Result<u8, EzspError> {
    link.reset().await?;
    let wait = config.rst_timeout * config.rst_attempts + Duration::from_millis(500);
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Err(_) => return Err(EzspError::NoReply),
            Ok(None) => return Err(EzspError::Down("link task stopped".into())),
            Ok(Some(LinkEvent::Ash(AshEvent::Connected { reset_code }))) => return Ok(reset_code),
            Ok(Some(LinkEvent::Ash(AshEvent::Down(zigbee_ash::DownReason::ResetTimeout)))) => {
                return Err(EzspError::NoReply)
            }
            Ok(Some(LinkEvent::Ash(AshEvent::Down(reason)))) => {
                return Err(EzspError::Down(format!("{reason:?}")))
            }
            Ok(Some(LinkEvent::Closed(why))) => return Err(EzspError::Down(why)),
            Ok(Some(LinkEvent::Ash(AshEvent::Data(_)))) => {}
        }
    }
}

/// Reset the NCP's ASH layer and report the RSTACK code: the one frame
/// [`Ezsp::start`] begins with, and the cheapest proof that an EZSP NCP is on
/// the port. Returns [`EzspError::NoReply`] when nothing answers.
pub async fn ash_reset<T>(io: T, config: &AshConfig) -> Result<u8, EzspError>
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    let (link, mut events) = Link::spawn(io, Role::Host, config.clone());
    connect_link(&link, &mut events, config).await
}

impl Ezsp {
    /// Connect over `io`: reset the link, negotiate the protocol version
    /// and read the stack version. Callbacks arrive on the receiver; it is
    /// unbounded so a slow consumer never stalls command responses.
    pub async fn start<T>(
        io: T,
        config: ClientConfig,
    ) -> Result<(Ezsp, NcpInfo, mpsc::UnboundedReceiver<EzspEvent>), EzspError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (link, mut link_events) = Link::spawn(io, Role::Host, config.ash.clone());
        let reset_code = connect_link(&link, &mut link_events, &config.ash).await?;
        let inner = Arc::new(Inner {
            link,
            version: AtomicU8::new(MAX_PROTOCOL_VERSION),
            extended: AtomicBool::new(false),
            down: Mutex::new(None),
            seq: tokio::sync::Mutex::new(0),
            pending: Mutex::new(None),
            timeout: config.response_timeout,
        });
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        tokio::spawn(reader(inner.clone(), link_events, event_tx));
        let ezsp = Ezsp { inner };

        // First `version` in the legacy format, asking for the newest we speak.
        let (protocol, stack_type, packed) = ezsp.version_command(MAX_PROTOCOL_VERSION).await?;
        if stack_type != ids::STACK_TYPE_MESH {
            return Err(EzspError::StackType(stack_type));
        }
        if !ProtocolVersion(protocol).is_supported() {
            return Err(EzspError::Unsupported {
                protocol,
                stack_version: packed,
            });
        }
        ezsp.inner.version.store(protocol, Ordering::SeqCst);
        ezsp.inner.extended.store(true, Ordering::SeqCst);
        // Again in the extended format, now at the NCP's version.
        let (agreed, _, packed) = ezsp.version_command(protocol).await?;
        if agreed != protocol {
            return Err(EzspError::Unsupported {
                protocol: agreed,
                stack_version: packed,
            });
        }
        let stack_version = ezsp
            .get_value(ids::value::VERSION_INFO)
            .await
            .and_then(|bytes| {
                StackVersion::read(&mut Reader::new(&bytes))
                    .map_err(|e| EzspError::Decode(DecodeError::Truncated(e)))
            })?;
        let info = NcpInfo {
            protocol: ProtocolVersion(protocol),
            stack_type,
            stack_version_packed: packed,
            stack_version,
            reset_code,
        };
        Ok((ezsp, info, event_rx))
    }

    async fn version_command(&self, desired: u8) -> Result<(u8, u8, u16), EzspError> {
        match self.call(Command::Version { desired }).await? {
            Response::Version {
                protocol_version,
                stack_type,
                stack_version,
            } => Ok((protocol_version, stack_type, stack_version)),
            other => Err(EzspError::Unexpected(Box::new(other))),
        }
    }

    /// The negotiated protocol version.
    pub fn version(&self) -> ProtocolVersion {
        ProtocolVersion(self.inner.version.load(Ordering::SeqCst))
    }

    /// Send a command and wait for its response.
    pub async fn call(&self, command: Command) -> Result<Response, EzspError> {
        let frame_id = command.frame_id();
        let mut seq = self.inner.seq.lock().await;
        if let Some(why) = self.inner.down.lock().ok().and_then(|d| d.clone()) {
            return Err(EzspError::Down(why));
        }
        let this_seq = *seq;
        *seq = seq.wrapping_add(1);
        let format = if self.inner.extended.load(Ordering::SeqCst) {
            Format::Extended
        } else {
            Format::Legacy
        };
        let bytes = frame::encode(
            format,
            this_seq,
            0x00,
            frame_id,
            &command.encode(self.version()),
        );
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.inner.pending.lock() {
            *pending = Some(Pending {
                seq: this_seq,
                frame_id,
                reply: tx,
            });
        }
        tracing::trace!(frame_id, seq = this_seq, "ezsp tx");
        self.inner.link.send(bytes).await?;
        let result = match tokio::time::timeout(self.inner.timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(EzspError::Down(self.down_reason())),
            Err(_) => Err(EzspError::Timeout(frame_id)),
        };
        if let Ok(mut pending) = self.inner.pending.lock() {
            *pending = None;
        }
        drop(seq);
        match result? {
            Response::InvalidCommand { reason } => {
                Err(EzspError::InvalidCommand { frame_id, reason })
            }
            response => Ok(response),
        }
    }

    fn down_reason(&self) -> String {
        self.inner
            .down
            .lock()
            .ok()
            .and_then(|d| d.clone())
            .unwrap_or_else(|| "link closed".into())
    }

    /// Send a command whose response is a status and require success.
    pub async fn call_ok(&self, command: Command) -> Result<(), EzspError> {
        let frame_id = command.frame_id();
        let response = self.call(command).await?;
        match response.status() {
            Some(status) if status.is_ok() => Ok(()),
            Some(status) => Err(EzspError::Status { frame_id, status }),
            None => Ok(()),
        }
    }

    /// `getValue`, requiring success.
    pub async fn get_value(&self, value_id: u8) -> Result<Vec<u8>, EzspError> {
        match self.call(Command::GetValue { value_id }).await? {
            Response::GetValue { status, value } if status.is_ok() => Ok(value),
            Response::GetValue { status, .. } => Err(EzspError::Status {
                frame_id: ids::frame::GET_VALUE,
                status,
            }),
            other => Err(EzspError::Unexpected(Box::new(other))),
        }
    }

    /// `setValue`, requiring success.
    pub async fn set_value(&self, value_id: u8, value: Vec<u8>) -> Result<(), EzspError> {
        self.call_ok(Command::SetValue { value_id, value }).await
    }

    /// `setConfigurationValue`, requiring success.
    pub async fn set_config(&self, config_id: u8, value: u16) -> Result<(), EzspError> {
        self.call_ok(Command::SetConfigurationValue { config_id, value })
            .await
    }

    /// `getConfigurationValue`, requiring success.
    pub async fn get_config(&self, config_id: u8) -> Result<u16, EzspError> {
        match self
            .call(Command::GetConfigurationValue { config_id })
            .await?
        {
            Response::GetConfigurationValue { status, value } if status.is_ok() => Ok(value),
            Response::GetConfigurationValue { status, .. } => Err(EzspError::Status {
                frame_id: ids::frame::GET_CONFIGURATION_VALUE,
                status,
            }),
            other => Err(EzspError::Unexpected(Box::new(other))),
        }
    }

    /// `setPolicy`, requiring success.
    pub async fn set_policy(&self, policy_id: u8, decision: u8) -> Result<(), EzspError> {
        self.call_ok(Command::SetPolicy {
            policy_id,
            decision,
        })
        .await
    }
}

async fn reader(
    inner: Arc<Inner>,
    mut link_events: mpsc::Receiver<LinkEvent>,
    events: mpsc::UnboundedSender<EzspEvent>,
) {
    let reason = loop {
        let Some(event) = link_events.recv().await else {
            break "link task stopped".to_string();
        };
        let data = match event {
            LinkEvent::Ash(AshEvent::Data(data)) => data,
            LinkEvent::Ash(AshEvent::Connected { .. }) => continue,
            LinkEvent::Ash(AshEvent::Down(reason)) => break format!("{reason:?}"),
            LinkEvent::Closed(why) => break why,
        };
        let format = if inner.extended.load(Ordering::SeqCst) {
            Format::Extended
        } else {
            Format::Legacy
        };
        let version = ProtocolVersion(inner.version.load(Ordering::SeqCst));
        let header = match frame::parse(&data, format) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(error = %e, "dropping malformed EZSP frame");
                continue;
            }
        };
        if !header.is_response() {
            tracing::warn!(
                frame_id = header.frame_id,
                "dropping EZSP command frame from the NCP"
            );
            continue;
        }
        if header.overflow() {
            tracing::warn!("NCP reports it ran out of memory (overflow)");
        }
        if header.is_callback() {
            match Callback::decode(version, header.frame_id, header.params) {
                Ok(cb) => {
                    tracing::trace!(?cb, "ezsp callback");
                    let _ = events.send(EzspEvent::Callback(cb));
                }
                Err(e) => {
                    tracing::warn!(frame_id = header.frame_id, error = %e, "dropping malformed EZSP callback")
                }
            }
            continue;
        }
        let pending = inner.pending.lock().ok().and_then(|mut p| p.take());
        let Some(pending) = pending else {
            tracing::warn!(
                frame_id = header.frame_id,
                seq = header.seq,
                "EZSP response with no command waiting"
            );
            continue;
        };
        if header.seq != pending.seq {
            tracing::warn!(
                want = pending.seq,
                got = header.seq,
                "EZSP response out of sequence"
            );
            if let Ok(mut p) = inner.pending.lock() {
                *p = Some(pending);
            }
            continue;
        }
        let result = if header.truncated() {
            Err(EzspError::Decode(DecodeError::Truncated(
                zigbee_types::wire::Truncated {
                    offset: 0,
                    needed: 0,
                },
            )))
        } else if header.frame_id == ids::frame::INVALID_COMMAND
            || header.frame_id == pending.frame_id
        {
            Response::decode(version, header.frame_id, header.params).map_err(EzspError::from)
        } else {
            Err(EzspError::Decode(DecodeError::UnknownFrame(
                header.frame_id,
            )))
        };
        let _ = pending.reply.send(result);
    };
    tracing::warn!(%reason, "EZSP link down");
    if let Ok(mut down) = inner.down.lock() {
        *down = Some(reason.clone());
    }
    if let Some(p) = inner.pending.lock().ok().and_then(|mut p| p.take()) {
        let _ = p.reply.send(Err(EzspError::Down(reason.clone())));
    }
    let _ = events.send(EzspEvent::Down(reason));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks_stack_versions() {
        assert_eq!(unpack_stack_version(0x7440), (7, 4, 4, 0));
        assert_eq!(unpack_stack_version(0x6A30), (6, 10, 3, 0));
    }
}
