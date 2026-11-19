//! Open a coordinator's port by name: a serial device, or `tcp://host:port`
//! for network-attached coordinators and the simulator.

use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Serial flow control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlowControl {
    /// None (most USB coordinators with EmberZNet's default firmware).
    #[default]
    None,
    /// RTS/CTS hardware flow control.
    Hardware,
    /// XON/XOFF software flow control.
    Software,
}

/// An open coordinator port.
pub enum Port {
    /// A serial device.
    Serial(tokio_serial::SerialStream),
    /// A TCP connection.
    Tcp(tokio::net::TcpStream),
}

impl std::fmt::Debug for Port {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Port::Serial(_) => f.write_str("Port::Serial"),
            Port::Tcp(_) => f.write_str("Port::Tcp"),
        }
    }
}

/// Open `name`: `tcp://host:port`, or a serial device path at `baud`.
pub async fn open(name: &str, baud: u32, flow: FlowControl) -> std::io::Result<Port> {
    if let Some(addr) = name.strip_prefix("tcp://") {
        let stream = tokio::net::TcpStream::connect(addr).await?;
        stream.set_nodelay(true)?;
        return Ok(Port::Tcp(stream));
    }
    use tokio_serial::SerialPortBuilderExt;
    let flow = match flow {
        FlowControl::None => tokio_serial::FlowControl::None,
        FlowControl::Hardware => tokio_serial::FlowControl::Hardware,
        FlowControl::Software => tokio_serial::FlowControl::Software,
    };
    let stream = tokio_serial::new(name, baud)
        .data_bits(tokio_serial::DataBits::Eight)
        .parity(tokio_serial::Parity::None)
        .stop_bits(tokio_serial::StopBits::One)
        .flow_control(flow)
        .open_native_async()
        .map_err(std::io::Error::from)?;
    Ok(Port::Serial(stream))
}

impl AsyncRead for Port {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Port::Serial(s) => Pin::new(s).poll_read(cx, buf),
            Port::Tcp(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Port {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Port::Serial(s) => Pin::new(s).poll_write(cx, buf),
            Port::Tcp(s) => Pin::new(s).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Port::Serial(s) => Pin::new(s).poll_flush(cx),
            Port::Tcp(s) => Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Port::Serial(s) => Pin::new(s).poll_shutdown(cx),
            Port::Tcp(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}
