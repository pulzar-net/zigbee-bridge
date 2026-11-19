//! A tokio task that runs a [`Connection`] over any byte stream.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::conn::{Config, Connection, Event, Role, SendError};
use crate::frame::Decoder;

/// What the link task reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEvent {
    /// An event from the link layer.
    Ash(Event),
    /// The byte stream ended or failed; the task has stopped.
    Closed(String),
}

/// The link task has stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    /// The task is gone.
    #[error("ASH link task has stopped")]
    Closed,
    /// The connection refused the payload.
    #[error(transparent)]
    Send(#[from] SendError),
}

enum Command {
    Send(Vec<u8>, tokio::sync::oneshot::Sender<Result<(), SendError>>),
    Reset,
}

/// A handle to a running link task. Cloning shares the task.
#[derive(Clone, Debug)]
pub struct Link {
    commands: mpsc::Sender<Command>,
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::Send(p, _) => write!(f, "Send({} bytes)", p.len()),
            Command::Reset => f.write_str("Reset"),
        }
    }
}

impl Link {
    /// Start a link task over `io`. Events arrive on the returned receiver;
    /// a host link must be started with [`Link::reset`].
    pub fn spawn<T>(io: T, role: Role, config: Config) -> (Link, mpsc::Receiver<LinkEvent>)
    where
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (commands, command_rx) = mpsc::channel(16);
        let (event_tx, events) = mpsc::channel(64);
        tokio::spawn(run(io, Connection::new(role, config), command_rx, event_tx));
        (Link { commands }, events)
    }

    /// Queue an EZSP frame. Returns once the link has accepted it (not
    /// once the peer acknowledged it).
    pub async fn send(&self, payload: Vec<u8>) -> Result<(), LinkError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.commands
            .send(Command::Send(payload, tx))
            .await
            .map_err(|_| LinkError::Closed)?;
        rx.await
            .map_err(|_| LinkError::Closed)?
            .map_err(LinkError::Send)
    }

    /// Host: reset the link (sends RST). [`Event::Connected`] follows.
    pub async fn reset(&self) -> Result<(), LinkError> {
        self.commands
            .send(Command::Reset)
            .await
            .map_err(|_| LinkError::Closed)
    }
}

async fn run<T>(
    io: T,
    mut conn: Connection,
    mut commands: mpsc::Receiver<Command>,
    events: mpsc::Sender<LinkEvent>,
) where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(io);
    let mut decoder = Decoder::new();
    let mut buf = [0u8; 256];
    let far = Duration::from_secs(3600);
    loop {
        let mut wrote = false;
        while let Some(bytes) = conn.poll_transmit() {
            if let Err(e) = writer.write_all(&bytes).await {
                let _ = events
                    .send(LinkEvent::Closed(format!("write failed: {e}")))
                    .await;
                return;
            }
            wrote = true;
        }
        if wrote {
            if let Err(e) = writer.flush().await {
                let _ = events
                    .send(LinkEvent::Closed(format!("flush failed: {e}")))
                    .await;
                return;
            }
        }
        while let Some(event) = conn.poll_event() {
            if events.send(LinkEvent::Ash(event)).await.is_err() {
                return;
            }
        }
        let deadline = conn
            .poll_timeout()
            .map(Instant::from_std)
            .unwrap_or_else(|| Instant::now() + far);
        tokio::select! {
            read = reader.read(&mut buf) => match read {
                Ok(0) => {
                    let _ = events.send(LinkEvent::Closed("end of stream".into())).await;
                    return;
                }
                Ok(n) => {
                    let now = Instant::now().into_std();
                    for byte in buf.iter().take(n) {
                        match decoder.push(*byte) {
                            Some(Ok(frame)) => {
                                tracing::trace!(?frame, "ash rx");
                                conn.handle_frame(frame, now);
                            }
                            Some(Err(error)) => {
                                tracing::debug!(%error, "ash rx dropped frame");
                                conn.handle_bad_frame(error, now);
                            }
                            None => {}
                        }
                    }
                }
                Err(e) => {
                    let _ = events.send(LinkEvent::Closed(format!("read failed: {e}"))).await;
                    return;
                }
            },
            command = commands.recv() => match command {
                Some(Command::Send(payload, reply)) => {
                    let _ = reply.send(conn.send(payload, Instant::now().into_std()));
                }
                Some(Command::Reset) => {
                    decoder.reset();
                    conn.reset(Instant::now().into_std());
                }
                None => return,
            },
            _ = tokio::time::sleep_until(deadline) => {
                conn.handle_timeout(Instant::now().into_std());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn host_and_ncp_talk_over_a_duplex_stream() {
        let (a, b) = tokio::io::duplex(1024);
        let (host, mut host_events) = Link::spawn(a, Role::Host, Config::default());
        let (ncp, mut ncp_events) = Link::spawn(b, Role::Ncp, Config::default());
        host.reset().await.unwrap();
        assert!(matches!(
            host_events.recv().await,
            Some(LinkEvent::Ash(Event::Connected { .. }))
        ));
        assert!(matches!(
            ncp_events.recv().await,
            Some(LinkEvent::Ash(Event::Connected { .. }))
        ));
        host.send(vec![1, 2, 3, 4]).await.unwrap();
        assert_eq!(
            ncp_events.recv().await,
            Some(LinkEvent::Ash(Event::Data(vec![1, 2, 3, 4])))
        );
        ncp.send(vec![5, 6, 7]).await.unwrap();
        assert_eq!(
            host_events.recv().await,
            Some(LinkEvent::Ash(Event::Data(vec![5, 6, 7])))
        );
    }
}
