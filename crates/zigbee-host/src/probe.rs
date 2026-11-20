//! What NCP is on a port.
//!
//! - EZSP: one ASH reset (RST, which every EZSP session starts with and
//!   which only resets the NCP's serial link state machine and stack, never
//!   its network), then read-only queries: `version`, the `VERSION_INFO`
//!   value, the coordinator's IEEE address and the board's manufacturing
//!   strings.
//! - ZNP: the CC2652 bootloader skip byte, `SYS_PING`, `SYS_VERSION` and
//!   `UTIL_GET_DEVICE_INFO`.
//!
//! Nothing is written. A probe finishes in under three seconds and drops
//! the port when it returns.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use zigbee_ash::Config as AshConfig;
use zigbee_ezsp::client::{unpack_stack_version, ClientConfig, Ezsp, EzspError};
use zigbee_ezsp::{ids, Command, Response};
use zigbee_types::Ieee;

/// The NCP families a port can be probed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NcpKind {
    /// Silicon Labs EmberZNet (EZSP over ASH).
    Ezsp,
    /// Texas Instruments Z-Stack (ZNP).
    Znp,
}

impl NcpKind {
    /// Every family, in the order [`probe_any`] tries them.
    pub const ALL: [NcpKind; 2] = [NcpKind::Ezsp, NcpKind::Znp];

    /// The lower-case name: `ezsp`, `znp`.
    pub fn name(self) -> &'static str {
        match self {
            NcpKind::Ezsp => "ezsp",
            NcpKind::Znp => "znp",
        }
    }
}

impl std::str::FromStr for NcpKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        NcpKind::ALL
            .into_iter()
            .find(|k| k.name() == s)
            .ok_or_else(|| format!("unknown coordinator family {s:?} (ezsp or znp)"))
    }
}

/// What a probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NcpIdentity {
    /// The family.
    pub kind: NcpKind,
    /// Firmware version, such as `7.4.4.0` (EmberZNet) or
    /// `2.7.1.20230507` (Z-Stack major.minor.maintenance.build).
    pub firmware: String,
    /// The serial protocol version (EZSP) or transport revision (ZNP).
    pub protocol: u8,
    /// Whether this host speaks that protocol version.
    pub supported: bool,
    /// Why the NCP last reset (RSTACK code; 0 for ZNP).
    pub reset_code: u8,
    /// The coordinator's IEEE address, when the protocol is supported.
    pub ieee: Option<Ieee>,
    /// The manufacturer string token, if set (often names the vendor;
    /// EZSP only).
    pub manufacturer: Option<String>,
    /// The board name token, if set (often names the product or chip;
    /// EZSP only).
    pub board: Option<String>,
    /// The firmware product: `Z-Stack 3.x.0` (CC13x2, CC26x2, CC2652),
    /// `Z-Stack 3.0.x` (CC2530, CC2531) or `Z-Stack 1.2` (ZNP only).
    pub product: Option<String>,
}

/// The probe could not talk to the port at all.
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    /// Opening, reading or writing the port failed.
    #[error("port I/O: {0}")]
    Io(#[from] std::io::Error),
}

/// The longest a probe takes.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

fn probe_config() -> ClientConfig {
    ClientConfig {
        ash: AshConfig {
            rst_timeout: Duration::from_millis(1100),
            rst_attempts: 2,
            ..AshConfig::default()
        },
        response_timeout: Duration::from_millis(600),
    }
}

fn token_string(bytes: &[u8]) -> Option<String> {
    let text: String = bytes
        .iter()
        .take_while(|b| **b != 0x00 && **b != 0xFF)
        .map(|b| *b as char)
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .collect();
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

async fn mfg_string(ezsp: &Ezsp, token_id: u8) -> Option<String> {
    match ezsp.call(Command::GetMfgToken { token_id }).await {
        Ok(Response::GetMfgToken { data }) => token_string(&data),
        _ => None,
    }
}

/// Probe an open stream for an EZSP NCP. `Ok(None)` means nothing valid
/// answered (wrong device, wrong baud rate); `Err` is an I/O failure.
pub async fn probe_ezsp<T>(io: T) -> Result<Option<NcpIdentity>, ProbeError>
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    let result = tokio::time::timeout(PROBE_TIMEOUT, Ezsp::start(io, probe_config())).await;
    let (ezsp, info, _events) = match result {
        Err(_) => return Ok(None),
        Ok(Ok(started)) => started,
        Ok(Err(EzspError::Unsupported {
            protocol,
            stack_version,
        })) => {
            let (a, b, c, d) = unpack_stack_version(stack_version);
            return Ok(Some(NcpIdentity {
                kind: NcpKind::Ezsp,
                firmware: format!("{a}.{b}.{c}.{d}"),
                protocol,
                supported: false,
                reset_code: 0,
                ieee: None,
                manufacturer: None,
                board: None,
                product: None,
            }));
        }
        Ok(Err(EzspError::Down(why)))
            if why.contains("read failed") || why.contains("write failed") =>
        {
            return Err(ProbeError::Io(std::io::Error::other(why)));
        }
        Ok(Err(_)) => return Ok(None),
    };
    let v = info.stack_version;
    let ieee = match ezsp.call(Command::GetEui64).await {
        Ok(Response::GetEui64 { ieee }) => Some(ieee),
        _ => None,
    };
    let manufacturer = mfg_string(&ezsp, ids::mfg_token::STRING).await;
    let board = mfg_string(&ezsp, ids::mfg_token::BOARD_NAME).await;
    Ok(Some(NcpIdentity {
        kind: NcpKind::Ezsp,
        firmware: format!("{}.{}.{}.{}", v.major, v.minor, v.patch, v.special),
        protocol: info.protocol.0,
        supported: true,
        reset_code: info.reset_code,
        ieee,
        manufacturer,
        board,
        product: None,
    }))
}

/// Probe an open stream for a Z-Stack ZNP coordinator. `Ok(None)` means
/// nothing valid answered.
pub async fn probe_znp<T>(io: T) -> Result<Option<NcpIdentity>, ProbeError>
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    use zigbee_znp::client::{ClientConfig as ZnpConfig, Znp, ZnpError};
    use zigbee_znp::ids::product;
    let config = ZnpConfig {
        response_timeout: Duration::from_millis(500),
        ping_attempts: 2,
        skip_bootloader: true,
        bootloader_delay: Duration::from_millis(300),
    };
    let (znp, _events) = match tokio::time::timeout(PROBE_TIMEOUT, Znp::open(io, config)).await {
        Err(_) => return Ok(None),
        Ok(Ok(opened)) => opened,
        Ok(Err(ZnpError::Down(why))) if why.starts_with("read:") => {
            return Err(ProbeError::Io(std::io::Error::other(why)))
        }
        Ok(Err(_)) => return Ok(None),
    };
    let v = znp.version();
    let ieee = if v.is_supported() {
        match znp.request_ok(zigbee_znp::Request::UtilGetDeviceInfo).await {
            Ok(data) => zigbee_types::wire::Reader::new(data.get(1..).unwrap_or(&[]))
                .ieee()
                .ok(),
            Err(_) => None,
        }
    } else {
        None
    };
    let name = match v.product {
        product::ZSTACK_3X0 => "Z-Stack 3.x.0",
        product::ZSTACK_30X => "Z-Stack 3.0.x",
        product::ZSTACK_12 => "Z-Stack 1.2",
        _ => "Z-Stack",
    };
    Ok(Some(NcpIdentity {
        kind: NcpKind::Znp,
        firmware: v.firmware(),
        protocol: v.transport,
        supported: v.is_supported(),
        reset_code: 0,
        ieee,
        manufacturer: None,
        board: None,
        product: Some(name.into()),
    }))
}

/// Probe an open stream for `kind`.
pub async fn probe_stream<T>(io: T, kind: NcpKind) -> Result<Option<NcpIdentity>, ProbeError>
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    match kind {
        NcpKind::Ezsp => probe_ezsp(io).await,
        NcpKind::Znp => probe_znp(io).await,
    }
}

/// Open `port` (a serial device or `tcp://host:port`) at `baud` and probe
/// it for `kind`. The port is closed when this returns.
#[cfg(feature = "serial")]
pub async fn probe(
    port: &str,
    kind: NcpKind,
    baud: u32,
) -> Result<Option<NcpIdentity>, ProbeError> {
    let io = crate::port::open(port, baud, crate::port::FlowControl::None).await?;
    probe_stream(io, kind).await
}

/// Probe `port` for each family in turn ([`NcpKind::ALL`]), opening it
/// afresh for each; the first that answers wins.
#[cfg(feature = "serial")]
pub async fn probe_any(port: &str, baud: u32) -> Result<Option<NcpIdentity>, ProbeError> {
    for kind in NcpKind::ALL {
        if let Some(found) = probe(port, kind, baud).await? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_become_text() {
        assert_eq!(
            token_string(b"Nabu Casa\0\xff\xff"),
            Some("Nabu Casa".into())
        );
        assert_eq!(token_string(&[0xFF; 16]), None);
    }

    #[tokio::test(start_paused = true)]
    async fn silence_is_no_reply_not_an_error() {
        let (a, _b) = tokio::io::duplex(64);
        let started = tokio::time::Instant::now();
        assert!(probe_ezsp(a).await.unwrap().is_none());
        assert!(started.elapsed() <= PROBE_TIMEOUT);
    }
}
