//! An async ZNP client: one synchronous request at a time, as MT requires,
//! and every asynchronous frame as an event.

use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use zigbee_types::wire::Reader;

use crate::frame::{Decoded, Decoder, Frame, FrameError, Kind};
use crate::ids::{self, subsystem};
use crate::message::{DecodeError, Indication, Request};

/// The serial bootloader on CC2652 sticks waits a few seconds for an image
/// unless it sees this byte first.
const SKIP_BOOTLOADER: u8 = 0xEF;
/// The longest an OSAL NV read returns per request.
const NV_CHUNK: usize = 240;

/// Client errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ZnpError {
    /// The port closed or failed.
    #[error("ZNP port down: {0}")]
    Down(String),
    /// Nothing answered `SYS_PING`.
    #[error("no ZNP reply (is this a Z-Stack coordinator at the right baud rate?)")]
    NoReply,
    /// No response in time.
    #[error("no response to {subsystem:#04x} {id:#04x}")]
    Timeout {
        /// Subsystem.
        subsystem: u8,
        /// Command.
        id: u8,
    },
    /// The NCP does not know the command.
    #[error("the NCP rejected {subsystem:#04x} {id:#04x} (RPC error {code:#04x})")]
    Rpc {
        /// Subsystem.
        subsystem: u8,
        /// Command.
        id: u8,
        /// 1 invalid subsystem, 2 invalid command, 3 invalid parameter, 4 invalid length.
        code: u8,
    },
    /// The command returned a failure status.
    #[error("{subsystem:#04x} {id:#04x} failed with status {status:#04x}")]
    Status {
        /// Subsystem.
        subsystem: u8,
        /// Command.
        id: u8,
        /// Status.
        status: u8,
    },
    /// A response did not parse.
    #[error("bad ZNP response: {0}")]
    Decode(#[from] DecodeError),
    /// A request did not encode.
    #[error(transparent)]
    Frame(#[from] FrameError),
    /// The firmware is not Z-Stack 3.
    #[error("Z-Stack product {product} {major}.{minor}.{maint} is not supported: need Z-Stack 3.0.x or 3.x.0 (Zigbee 3.0)")]
    Unsupported {
        /// Product id.
        product: u8,
        /// Major release.
        major: u8,
        /// Minor release.
        minor: u8,
        /// Maintenance release.
        maint: u8,
    },
}

impl From<zigbee_types::wire::Truncated> for ZnpError {
    fn from(e: zigbee_types::wire::Truncated) -> Self {
        ZnpError::Decode(DecodeError::Truncated(e))
    }
}

/// `SYS_VERSION`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    /// Transport revision.
    pub transport: u8,
    /// Product ([`ids::product`]).
    pub product: u8,
    /// Major release.
    pub major: u8,
    /// Minor release.
    pub minor: u8,
    /// Maintenance release.
    pub maint: u8,
    /// Firmware build, usually a date such as 20230507.
    pub revision: Option<u32>,
}

impl Version {
    /// Parse the response.
    pub fn parse(data: &[u8]) -> Result<Version, DecodeError> {
        let mut r = Reader::new(data);
        Ok(Version {
            transport: r.u8()?,
            product: r.u8()?,
            major: r.u8()?,
            minor: r.u8()?,
            maint: r.u8()?,
            revision: if r.remaining() >= 4 {
                Some(r.u32()?)
            } else {
                None
            },
        })
    }

    /// Z-Stack 3.0.x or 3.x.0.
    pub fn is_supported(&self) -> bool {
        matches!(
            self.product,
            ids::product::ZSTACK_3X0 | ids::product::ZSTACK_30X
        )
    }

    /// Extended NV tables (3.x.0) rather than legacy items (3.0.x).
    pub fn has_extended_nv(&self) -> bool {
        self.product == ids::product::ZSTACK_3X0
    }

    /// `3.30.00.20230507`-style text.
    pub fn firmware(&self) -> String {
        match self.revision {
            Some(rev) => format!("{}.{}.{}.{rev}", self.major, self.minor, self.maint),
            None => format!("{}.{}.{}", self.major, self.minor, self.maint),
        }
    }
}

/// Something the NCP sent on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZnpEvent {
    /// An indication.
    Indication(Indication),
    /// The port is down; every later request fails.
    Down(String),
}

/// Client settings.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// How long to wait for each synchronous response.
    pub response_timeout: Duration,
    /// Pings before giving up at start.
    pub ping_attempts: u32,
    /// Send the CC2652 bootloader skip byte first.
    pub skip_bootloader: bool,
    /// How long to wait after it.
    pub bootloader_delay: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            response_timeout: Duration::from_secs(6),
            ping_attempts: 3,
            skip_bootloader: true,
            bootloader_delay: Duration::from_millis(500),
        }
    }
}

struct Pending {
    subsystem: u8,
    id: u8,
    reply: oneshot::Sender<Result<Vec<u8>, ZnpError>>,
}

struct Inner {
    write: mpsc::Sender<Vec<u8>>,
    down: Mutex<Option<String>>,
    lock: tokio::sync::Mutex<()>,
    pending: Mutex<Option<Pending>>,
    timeout: Duration,
    /// Dropped with the client, which stops the reader and closes the port.
    _close: oneshot::Sender<()>,
}

/// A ZNP client. Clones share one connection.
#[derive(Clone)]
pub struct Znp {
    inner: Arc<Inner>,
    version: Version,
}

impl std::fmt::Debug for Znp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Znp")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl Znp {
    /// Connect over `io`: skip the bootloader, ping, and read the version,
    /// requiring Z-Stack 3. Indications arrive on the receiver; it is
    /// unbounded so a slow consumer never stalls responses.
    pub async fn start<T>(
        io: T,
        config: ClientConfig,
    ) -> Result<(Znp, mpsc::UnboundedReceiver<ZnpEvent>), ZnpError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (znp, events) = Znp::open(io, config).await?;
        let version = znp.version;
        if !version.is_supported() {
            return Err(ZnpError::Unsupported {
                product: version.product,
                major: version.major,
                minor: version.minor,
                maint: version.maint,
            });
        }
        Ok((znp, events))
    }

    /// Connect over `io` and read the version without requiring a
    /// supported product: what a probe needs.
    pub async fn open<T>(
        io: T,
        config: ClientConfig,
    ) -> Result<(Znp, mpsc::UnboundedReceiver<ZnpEvent>), ZnpError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (inner, events) = spawn(io, &config);
        if config.skip_bootloader {
            let _ = inner.write.send(vec![SKIP_BOOTLOADER]).await;
            tokio::time::sleep(config.bootloader_delay).await;
        }
        let mut znp = Znp {
            inner,
            version: Version {
                transport: 0,
                product: 0,
                major: 0,
                minor: 0,
                maint: 0,
                revision: None,
            },
        };
        let mut answered = false;
        for _ in 0..config.ping_attempts.max(1) {
            match znp.request(Request::SysPing).await {
                Ok(_) => {
                    answered = true;
                    break;
                }
                Err(ZnpError::Timeout { .. }) => continue,
                Err(e) => return Err(e),
            }
        }
        if !answered {
            return Err(ZnpError::NoReply);
        }
        znp.version = Version::parse(&znp.request(Request::SysVersion).await?)?;
        Ok((znp, events))
    }

    /// The firmware's version.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Send a synchronous request and return its response data.
    pub async fn request(&self, request: Request) -> Result<Vec<u8>, ZnpError> {
        let frame = request.encode();
        let bytes = frame.encode()?;
        if frame.kind != Kind::Sreq {
            self.send(bytes).await?;
            return Ok(Vec::new());
        }
        let _one_at_a_time = self.inner.lock.lock().await;
        if let Some(why) = self.inner.down.lock().ok().and_then(|d| d.clone()) {
            return Err(ZnpError::Down(why));
        }
        let (tx, rx) = oneshot::channel();
        if let Ok(mut p) = self.inner.pending.lock() {
            *p = Some(Pending {
                subsystem: frame.subsystem,
                id: frame.id,
                reply: tx,
            });
        }
        tracing::trace!(subsystem = frame.subsystem, id = frame.id, "znp tx");
        self.send(bytes).await?;
        let result = match tokio::time::timeout(self.inner.timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(ZnpError::Down(self.down_reason())),
            Err(_) => Err(ZnpError::Timeout {
                subsystem: frame.subsystem,
                id: frame.id,
            }),
        };
        if let Ok(mut p) = self.inner.pending.lock() {
            *p = None;
        }
        result
    }

    /// A request whose response starts with a status byte; require success.
    pub async fn request_ok(&self, request: Request) -> Result<Vec<u8>, ZnpError> {
        let (subsystem, id) = request.command();
        let data = self.request(request).await?;
        match data.first() {
            Some(&ids::status::SUCCESS) | None => Ok(data),
            Some(&status) => Err(ZnpError::Status {
                subsystem,
                id,
                status,
            }),
        }
    }

    async fn send(&self, bytes: Vec<u8>) -> Result<(), ZnpError> {
        self.inner
            .write
            .send(bytes)
            .await
            .map_err(|_| ZnpError::Down(self.down_reason()))
    }

    fn down_reason(&self) -> String {
        self.inner
            .down
            .lock()
            .ok()
            .and_then(|d| d.clone())
            .unwrap_or_else(|| "port closed".into())
    }

    /// Read a whole OSAL NV item. `None` when it does not exist.
    pub async fn nv_read(&self, id: u16) -> Result<Option<Vec<u8>>, ZnpError> {
        let len = self.request(Request::OsalNvLength { id }).await?;
        let len = usize::from(Reader::new(&len).u16()?);
        if len == 0 {
            return Ok(None);
        }
        let mut value = Vec::with_capacity(len);
        while value.len() < len {
            let offset = u8::try_from(value.len()).map_err(|_| ZnpError::Status {
                subsystem: subsystem::SYS,
                id: ids::sys::OSAL_NV_READ,
                status: ids::status::NV_BAD_ITEM_LEN,
            })?;
            let data = self.request_ok(Request::OsalNvRead { id, offset }).await?;
            let mut r = Reader::new(&data);
            let _status = r.u8()?;
            let chunk = r.u8_prefixed()?;
            if chunk.is_empty() {
                break;
            }
            value.extend_from_slice(chunk);
        }
        value.truncate(len);
        Ok(Some(value))
    }

    /// Write an OSAL NV item, creating it at this length if needed.
    pub async fn nv_write(&self, id: u16, value: &[u8]) -> Result<(), ZnpError> {
        let len = u16::try_from(value.len()).unwrap_or(u16::MAX);
        let init = self
            .request(Request::OsalNvItemInit {
                id,
                len,
                init: value
                    .get(..value.len().min(NV_CHUNK))
                    .unwrap_or(value)
                    .to_vec(),
            })
            .await?;
        match init.first() {
            Some(&ids::status::SUCCESS) | Some(&ids::status::NV_ITEM_UNINIT) => {}
            Some(&status) => {
                return Err(ZnpError::Status {
                    subsystem: subsystem::SYS,
                    id: ids::sys::OSAL_NV_ITEM_INIT,
                    status,
                })
            }
            None => {}
        }
        for (i, chunk) in value.chunks(NV_CHUNK).enumerate() {
            let offset = u8::try_from(i * NV_CHUNK).map_err(|_| ZnpError::Status {
                subsystem: subsystem::SYS,
                id: ids::sys::OSAL_NV_WRITE,
                status: ids::status::NV_BAD_ITEM_LEN,
            })?;
            self.request_ok(Request::OsalNvWrite {
                id,
                offset,
                value: chunk.to_vec(),
            })
            .await?;
        }
        Ok(())
    }

    /// Read a whole extended NV item. `None` when it does not exist.
    pub async fn nv_ex_read(
        &self,
        sysid: u8,
        item: u16,
        sub: u16,
    ) -> Result<Option<Vec<u8>>, ZnpError> {
        let len = self.request(Request::NvLength { sysid, item, sub }).await?;
        let len = Reader::new(&len).u32()? as usize;
        if len == 0 {
            return Ok(None);
        }
        let mut value = Vec::with_capacity(len.min(4096));
        while value.len() < len {
            let want = (len - value.len()).min(NV_CHUNK);
            let data = self
                .request_ok(Request::NvRead {
                    sysid,
                    item,
                    sub,
                    offset: u16::try_from(value.len()).unwrap_or(u16::MAX),
                    len: want as u8,
                })
                .await?;
            let mut r = Reader::new(&data);
            let _status = r.u8()?;
            let chunk = r.u8_prefixed()?;
            if chunk.is_empty() {
                break;
            }
            value.extend_from_slice(chunk);
        }
        value.truncate(len);
        Ok(Some(value))
    }

    /// Write an existing extended NV item.
    pub async fn nv_ex_write(
        &self,
        sysid: u8,
        item: u16,
        sub: u16,
        value: &[u8],
    ) -> Result<(), ZnpError> {
        for (i, chunk) in value.chunks(NV_CHUNK).enumerate() {
            self.request_ok(Request::NvWrite {
                sysid,
                item,
                sub,
                offset: u16::try_from(i * NV_CHUNK).unwrap_or(u16::MAX),
                value: chunk.to_vec(),
            })
            .await?;
        }
        Ok(())
    }
}

fn spawn<T>(io: T, config: &ClientConfig) -> (Arc<Inner>, mpsc::UnboundedReceiver<ZnpEvent>)
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    let (read, mut write) = tokio::io::split(io);
    let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(16);
    let (close_tx, close_rx) = oneshot::channel();
    let inner = Arc::new(Inner {
        write: write_tx,
        down: Mutex::new(None),
        lock: tokio::sync::Mutex::new(()),
        pending: Mutex::new(None),
        timeout: config.response_timeout,
        _close: close_tx,
    });
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(bytes) = write_rx.recv().await {
            if write.write_all(&bytes).await.is_err() || write.flush().await.is_err() {
                break;
            }
        }
    });
    // The reader holds the client weakly: dropping the last `Znp` ends both
    // tasks, which closes the port.
    tokio::spawn(reader(Arc::downgrade(&inner), read, close_rx, event_tx));
    (inner, event_rx)
}

async fn reader<R>(
    weak: Weak<Inner>,
    mut read: R,
    mut close: oneshot::Receiver<()>,
    events: mpsc::UnboundedSender<ZnpEvent>,
) where
    R: AsyncRead + Unpin,
{
    let mut decoder = Decoder::new();
    let mut buf = [0u8; 256];
    let reason = loop {
        let read = tokio::select! {
            r = read.read(&mut buf) => r,
            _ = &mut close => return,
        };
        let n = match read {
            Ok(0) => break "port closed".to_string(),
            Ok(n) => n,
            Err(e) => break format!("read: {e}"),
        };
        let Some(inner) = weak.upgrade() else { return };
        for decoded in decoder.push(buf.get(..n).unwrap_or(&[])) {
            match decoded {
                Decoded::BadFcs => tracing::warn!("dropping ZNP frame with a bad check byte"),
                Decoded::Frame(frame) => handle(&inner, frame, &events),
            }
        }
    };
    tracing::warn!(%reason, "ZNP port down");
    let Some(inner) = weak.upgrade() else { return };
    if let Ok(mut d) = inner.down.lock() {
        *d = Some(reason.clone());
    }
    if let Some(p) = inner.pending.lock().ok().and_then(|mut p| p.take()) {
        let _ = p.reply.send(Err(ZnpError::Down(reason.clone())));
    }
    let _ = events.send(ZnpEvent::Down(reason));
}

fn handle(inner: &Inner, frame: Frame, events: &mpsc::UnboundedSender<ZnpEvent>) {
    match frame.kind {
        Kind::Srsp => {
            let pending = inner.pending.lock().ok().and_then(|mut p| p.take());
            let Some(pending) = pending else {
                tracing::warn!(
                    subsystem = frame.subsystem,
                    id = frame.id,
                    "ZNP response with no request waiting"
                );
                return;
            };
            if frame.subsystem == subsystem::RPC_ERROR && frame.id == 0 {
                let mut r = Reader::new(&frame.data);
                let code = r.u8().unwrap_or(0);
                let _ = pending.reply.send(Err(ZnpError::Rpc {
                    subsystem: pending.subsystem,
                    id: pending.id,
                    code,
                }));
            } else if frame.subsystem == pending.subsystem && frame.id == pending.id {
                let _ = pending.reply.send(Ok(frame.data));
            } else {
                tracing::warn!(
                    subsystem = frame.subsystem,
                    id = frame.id,
                    "ZNP response for another request"
                );
                if let Ok(mut p) = inner.pending.lock() {
                    *p = Some(pending);
                }
            }
        }
        Kind::Areq => match Indication::decode(&frame) {
            Ok(ind) => {
                tracing::trace!(?ind, "znp indication");
                let _ = events.send(ZnpEvent::Indication(ind));
            }
            Err(DecodeError::Unknown { .. }) => {
                tracing::trace!(
                    subsystem = frame.subsystem,
                    id = frame.id,
                    "ignoring ZNP indication"
                )
            }
            Err(e) => tracing::warn!(error = %e, "dropping malformed ZNP indication"),
        },
        Kind::Sreq | Kind::Poll => {
            tracing::warn!("dropping a request frame from the NCP")
        }
    }
}
