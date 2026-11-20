//! The OTA Upgrade cluster server: devices download images from the
//! directory in [`HostConfig::ota_dir`].
//!
//! Nothing is offered unless asked: a device's own queries are answered
//! "no image" (and reported, with what the directory holds for it), and a
//! download starts only after [`Host::ota_update`] for that device.

use super::*;
use crate::ota::{self, command as oc, status as os, OtaImage, OtaStore};

/// The largest block the server sends, whatever a device asks for; small
/// enough for every coordinator family's APS payload.
const MAX_BLOCK: u8 = 64;

pub(super) struct OtaSession {
    /// The device may download.
    update: bool,
    /// The image it is downloading.
    image: Option<OtaImage>,
    /// The version it runs.
    from: u32,
    /// Whoever waits for the device's query.
    waiter: Option<oneshot::Sender<OtaOffer>>,
    last_percent: u8,
}

/// A device's firmware and what the image directory has for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtaOffer {
    /// Manufacturer code.
    pub manufacturer: u16,
    /// Image type.
    pub image_type: u16,
    /// The file version it runs.
    pub current_version: u32,
    /// The newest newer version in the directory.
    pub available: Option<u32>,
    /// That image's size.
    pub size: Option<u32>,
}

fn u16_at(p: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(p.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(p: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(p.get(i..i + 4)?.try_into().ok()?))
}

impl<A: Adapter> Host<A> {
    fn ota_store(&self) -> Result<OtaStore, HostError> {
        self.inner
            .config
            .ota_dir
            .as_ref()
            .map(OtaStore::new)
            .ok_or_else(|| HostError::Invalid("no OTA image directory is configured".into()))
    }

    /// Ask a device which firmware it runs and whether the image directory
    /// has a newer one. The device is told to query (Image Notify) and
    /// answered "no image"; sleepy devices answer when they next wake.
    pub async fn ota_check(&self, ieee: Ieee) -> Result<OtaOffer, HostError> {
        self.ota_ask(ieee, false).await
    }

    /// Start upgrading a device from the image directory. Returns once the
    /// device took the offer; [`HostEvent::OtaProgress`] and
    /// [`HostEvent::OtaFinished`] follow.
    pub async fn ota_update(&self, ieee: Ieee) -> Result<OtaOffer, HostError> {
        let offer = self.ota_ask(ieee, true).await?;
        if offer.available.is_none() {
            return Err(HostError::Invalid(format!(
                "no newer image for {ieee} (manufacturer {:#06x}, image type {:#06x}, version {:#010x})",
                offer.manufacturer, offer.image_type, offer.current_version
            )));
        }
        Ok(offer)
    }

    async fn ota_ask(&self, ieee: Ieee, update: bool) -> Result<OtaOffer, HostError> {
        self.ota_store()?;
        let device = self.device(ieee).ok_or(HostError::UnknownDevice(ieee))?;
        let endpoint = device
            .endpoints
            .iter()
            .find(|e| e.output_clusters.contains(&ota::CLUSTER))
            .ok_or_else(|| HostError::Invalid(format!("{ieee} has no OTA Upgrade client")))?;
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state();
            if state.ota.get(&ieee).is_some_and(|s| s.image.is_some()) {
                return Err(HostError::Invalid(format!(
                    "{ieee} is already downloading an image"
                )));
            }
            state.ota.insert(
                ieee,
                OtaSession {
                    update,
                    image: None,
                    from: 0,
                    waiter: Some(tx),
                    last_percent: 0,
                },
            );
        }
        let mut notify = Frame::cluster(
            Direction::ServerToClient,
            self.next_seq(),
            oc::IMAGE_NOTIFY,
            // Payload type 0 (jitter only), jitter 100: query now.
            vec![0x00, 100],
        );
        notify.header.disable_default_response = true;
        let aps = Aps {
            profile: endpoint.profile_id,
            cluster: ota::CLUSTER,
            source_endpoint: HOST_ENDPOINT,
            destination_endpoint: endpoint.endpoint,
        };
        if let Err(e) = self
            .inner
            .adapter
            .send_unicast(device.nwk, aps, notify.encode())
            .await
        {
            self.state().ota.remove(&ieee);
            return Err(e);
        }
        match tokio::time::timeout(self.inner.config.ota_query_timeout, rx).await {
            Ok(Ok(offer)) => Ok(offer),
            _ => {
                let mut state = self.state();
                if state.ota.get(&ieee).is_some_and(|s| s.image.is_none()) {
                    state.ota.remove(&ieee);
                }
                Err(HostError::Timeout(device.nwk))
            }
        }
    }

    fn ota_reply(&self, source: NodeId, aps: Aps, seq: u8, command: u8, payload: Vec<u8>) {
        let mut f = Frame::cluster(Direction::ServerToClient, seq, command, payload);
        f.header.disable_default_response = true;
        self.send_reply(source, aps, f);
    }

    /// A client-to-server OTA command from a device.
    pub(super) fn on_ota(
        &self,
        ieee: Ieee,
        source: NodeId,
        aps: Aps,
        seq: u8,
        command: u8,
        p: &[u8],
    ) {
        match command {
            oc::QUERY_NEXT_IMAGE_REQUEST => self.ota_query(ieee, source, aps, seq, p),
            oc::IMAGE_BLOCK_REQUEST => {
                let (Some(offset), Some(&max)) = (u32_at(p, 9), p.get(13)) else {
                    return;
                };
                self.ota_blocks(ieee, source, aps, seq, p, offset, max, 1, 0);
            }
            oc::IMAGE_PAGE_REQUEST => {
                let (Some(offset), Some(&max), Some(page), Some(spacing)) =
                    (u32_at(p, 9), p.get(13), u16_at(p, 14), u16_at(p, 16))
                else {
                    return;
                };
                let blocks = u32::from(page).div_ceil(u32::from(max.clamp(1, MAX_BLOCK)));
                self.ota_blocks(ieee, source, aps, seq, p, offset, max, blocks, spacing);
            }
            oc::UPGRADE_END_REQUEST => self.ota_end(ieee, source, aps, seq, p),
            _ => {}
        }
    }

    fn ota_query(&self, ieee: Ieee, source: NodeId, aps: Aps, seq: u8, p: &[u8]) {
        let (Some(&fc), Some(manufacturer), Some(image_type), Some(current)) =
            (p.first(), u16_at(p, 1), u16_at(p, 3), u32_at(p, 5))
        else {
            return;
        };
        let hardware = if fc & 0x01 != 0 { u16_at(p, 9) } else { None };
        let image = self
            .ota_store()
            .ok()
            .and_then(|s| s.newer(manufacturer, image_type, current, hardware, ieee));
        let offer = OtaOffer {
            manufacturer,
            image_type,
            current_version: current,
            available: image.as_ref().map(|i| i.header.file_version),
            size: image.as_ref().map(|i| i.header.total_size),
        };
        let accept = {
            let mut state = self.state();
            let accept = match (state.ota.get_mut(&ieee), &image) {
                (Some(session), Some(image)) if session.update => {
                    session.image = Some(image.clone());
                    session.from = current;
                    session.last_percent = 0;
                    true
                }
                _ => false,
            };
            if let Some(session) = state.ota.get_mut(&ieee) {
                if let Some(waiter) = session.waiter.take() {
                    let _ = waiter.send(offer.clone());
                }
            }
            if !accept {
                state.ota.remove(&ieee);
            }
            accept
        };
        self.emit(HostEvent::OtaQuery {
            ieee,
            current_version: current,
            available: offer.available,
        });
        let payload = match (&image, accept) {
            (Some(image), true) => {
                tracing::info!(%ieee, from = format!("{current:#010x}"), to = format!("{:#010x}", image.header.file_version), "OTA upgrade starting");
                let mut v = vec![os::SUCCESS];
                v.extend_from_slice(&manufacturer.to_le_bytes());
                v.extend_from_slice(&image_type.to_le_bytes());
                v.extend_from_slice(&image.header.file_version.to_le_bytes());
                v.extend_from_slice(&image.header.total_size.to_le_bytes());
                v
            }
            _ => vec![os::NO_IMAGE_AVAILABLE],
        };
        self.ota_reply(source, aps, seq, oc::QUERY_NEXT_IMAGE_RESPONSE, payload);
    }

    #[allow(clippy::too_many_arguments)]
    fn ota_blocks(
        &self,
        ieee: Ieee,
        source: NodeId,
        aps: Aps,
        seq: u8,
        p: &[u8],
        offset: u32,
        max: u8,
        blocks: u32,
        spacing_ms: u16,
    ) {
        let (Some(manufacturer), Some(image_type), Some(version)) =
            (u16_at(p, 1), u16_at(p, 3), u32_at(p, 5))
        else {
            return;
        };
        let image = self
            .state()
            .ota
            .get(&ieee)
            .and_then(|s| s.image.clone())
            .filter(|i| {
                i.header.manufacturer == manufacturer
                    && i.header.image_type == image_type
                    && i.header.file_version == version
            });
        let Some(image) = image else {
            // Nothing offered (or the host restarted): stop the download.
            self.ota_reply(source, aps, seq, oc::IMAGE_BLOCK_RESPONSE, vec![os::ABORT]);
            return;
        };
        let size = image.header.total_size;
        let block = usize::from(max.clamp(1, MAX_BLOCK));
        let host = self.clone();
        tokio::spawn(async move {
            let mut offset = offset;
            for i in 0..blocks.min(256) {
                if offset >= size {
                    break;
                }
                if i > 0 && spacing_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(u64::from(spacing_ms.min(1000))))
                        .await;
                }
                let data = match image.read(offset, block).await {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!(%ieee, error = %e, "reading the OTA image");
                        host.ota_reply(source, aps, seq, oc::IMAGE_BLOCK_RESPONSE, vec![os::ABORT]);
                        return;
                    }
                };
                let mut v = vec![os::SUCCESS];
                v.extend_from_slice(&manufacturer.to_le_bytes());
                v.extend_from_slice(&image_type.to_le_bytes());
                v.extend_from_slice(&version.to_le_bytes());
                v.extend_from_slice(&offset.to_le_bytes());
                v.push(data.len() as u8);
                v.extend_from_slice(&data);
                host.ota_reply(source, aps, seq, oc::IMAGE_BLOCK_RESPONSE, v);
                offset += data.len() as u32;
                host.ota_progress(ieee, offset, size);
            }
        });
    }

    fn ota_progress(&self, ieee: Ieee, done: u32, size: u32) {
        let percent = (u64::from(done) * 100 / u64::from(size.max(1))).min(100) as u8;
        let report = {
            let mut state = self.state();
            match state.ota.get_mut(&ieee) {
                Some(s)
                    if percent >= s.last_percent.saturating_add(10)
                        || (percent == 100 && s.last_percent < 100) =>
                {
                    s.last_percent = percent;
                    true
                }
                _ => false,
            }
        };
        if report {
            self.emit(HostEvent::OtaProgress { ieee, percent });
        }
    }

    fn ota_end(&self, ieee: Ieee, source: NodeId, aps: Aps, seq: u8, p: &[u8]) {
        let (Some(&status), Some(manufacturer), Some(image_type), Some(version)) =
            (p.first(), u16_at(p, 1), u16_at(p, 3), u32_at(p, 5))
        else {
            return;
        };
        let Some(session) = self.state().ota.remove(&ieee) else {
            return;
        };
        let success = status == os::SUCCESS;
        if success {
            let mut v = Vec::with_capacity(16);
            v.extend_from_slice(&manufacturer.to_le_bytes());
            v.extend_from_slice(&image_type.to_le_bytes());
            v.extend_from_slice(&version.to_le_bytes());
            // Current time 0, upgrade time 0: apply now.
            v.extend_from_slice(&0u32.to_le_bytes());
            v.extend_from_slice(&0u32.to_le_bytes());
            self.ota_reply(source, aps, seq, oc::UPGRADE_END_RESPONSE, v);
            tracing::info!(%ieee, version = format!("{version:#010x}"), "OTA upgrade finished");
            // The new firmware may report and bind differently: interview
            // again once it has restarted.
            let host = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let _ = host.reinterview(ieee);
            });
        } else {
            tracing::warn!(%ieee, status, "OTA upgrade failed on the device");
        }
        self.emit(HostEvent::OtaFinished {
            ieee,
            from: session.from,
            to: version,
            success,
            status,
        });
    }
}
