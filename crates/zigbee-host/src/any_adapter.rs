//! An [`Adapter`] for whichever coordinator family is on the port, chosen
//! at run time.

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use zigbee_primitives::{Ieee, Key, NodeId};

use crate::adapter::{
    Adapter, AdapterEvent, AdapterInfo, Aps, Beacon, JoinPolicy, NetworkInfo, StartMode,
    StartOutcome,
};
use crate::backup::Backup;
use crate::deconz_adapter::DeconzAdapter;
use crate::error::HostError;
use crate::ezsp_adapter::EzspAdapter;
use crate::probe::NcpKind;
use crate::znp_adapter::ZnpAdapter;

/// One of the supported coordinator families.
#[derive(Debug)]
#[non_exhaustive]
pub enum AnyAdapter {
    /// Silicon Labs EmberZNet.
    Ezsp(EzspAdapter),
    /// Texas Instruments Z-Stack.
    Znp(ZnpAdapter),
    /// dresden elektronik deCONZ.
    Deconz(DeconzAdapter),
}

impl AnyAdapter {
    /// Connect to a `kind` coordinator over `io` with default settings.
    pub async fn connect<T>(
        io: T,
        kind: NcpKind,
    ) -> Result<(AnyAdapter, mpsc::UnboundedReceiver<AdapterEvent>), HostError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        match kind {
            NcpKind::Ezsp => {
                let (a, rx) =
                    EzspAdapter::connect(io, zigbee_ezsp::client::ClientConfig::default()).await?;
                Ok((AnyAdapter::Ezsp(a), rx))
            }
            NcpKind::Znp => {
                let (a, rx) =
                    ZnpAdapter::connect(io, zigbee_znp::client::ClientConfig::default()).await?;
                Ok((AnyAdapter::Znp(a), rx))
            }
            NcpKind::Deconz => {
                let (a, rx) =
                    DeconzAdapter::connect(io, zigbee_deconz::client::ClientConfig::default())
                        .await?;
                Ok((AnyAdapter::Deconz(a), rx))
            }
        }
    }

    /// The family.
    pub fn kind(&self) -> NcpKind {
        match self {
            AnyAdapter::Ezsp(_) => NcpKind::Ezsp,
            AnyAdapter::Znp(_) => NcpKind::Znp,
            AnyAdapter::Deconz(_) => NcpKind::Deconz,
        }
    }
}

macro_rules! each {
    ($self:ident, $a:ident => $e:expr) => {
        match $self {
            AnyAdapter::Ezsp($a) => $e,
            AnyAdapter::Znp($a) => $e,
            AnyAdapter::Deconz($a) => $e,
        }
    };
}

impl Adapter for AnyAdapter {
    fn info(&self) -> AdapterInfo {
        each!(self, a => a.info())
    }

    async fn start(&self, mode: StartMode) -> Result<(StartOutcome, NetworkInfo), HostError> {
        each!(self, a => a.start(mode).await)
    }

    async fn permit_join(&self, seconds: u8, policy: &JoinPolicy) -> Result<(), HostError> {
        each!(self, a => a.permit_join(seconds, policy).await)
    }

    async fn add_join_key(&self, ieee: Ieee, key: Key) -> Result<(), HostError> {
        each!(self, a => a.add_join_key(ieee, key).await)
    }

    async fn send_unicast(
        &self,
        nwk: NodeId,
        aps: Aps,
        payload: Vec<u8>,
    ) -> Result<u16, HostError> {
        each!(self, a => a.send_unicast(nwk, aps, payload).await)
    }

    async fn send_broadcast(
        &self,
        destination: NodeId,
        aps: Aps,
        payload: Vec<u8>,
    ) -> Result<(), HostError> {
        each!(self, a => a.send_broadcast(destination, aps, payload).await)
    }

    async fn network_info(&self) -> Result<NetworkInfo, HostError> {
        each!(self, a => a.network_info().await)
    }

    async fn backup(&self) -> Result<Backup, HostError> {
        each!(self, a => a.backup().await)
    }

    async fn active_scan(&self, channel_mask: u32) -> Result<Vec<Beacon>, HostError> {
        each!(self, a => a.active_scan(channel_mask).await)
    }

    async fn leave(&self) -> Result<(), HostError> {
        each!(self, a => a.leave().await)
    }

    async fn energy_scan(&self, channel_mask: u32) -> Result<Vec<(u8, i8)>, HostError> {
        each!(self, a => a.energy_scan(channel_mask).await)
    }
}
