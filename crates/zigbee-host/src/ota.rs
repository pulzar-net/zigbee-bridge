//! Zigbee OTA upgrade images and a directory of them.
//!
//! An OTA file (Zigbee OTA Upgrade cluster specification, section 11.4)
//! starts with a header naming the manufacturer, image type and file
//! version; a device downloads the whole file, header included, in blocks.
//! Some vendors wrap the file in a prefix of their own, so the header is
//! looked for in the first [`MAX_PREFIX`] bytes.
//!
//! [`OtaStore`] indexes a directory of such files by header only and reads
//! blocks from disk as devices ask for them, so an image never sits in
//! memory whole.

use std::path::{Path, PathBuf};

use zigbee_types::wire::{Reader, Truncated};
use zigbee_types::Ieee;

/// The OTA file identifier.
pub const MAGIC: u32 = 0x0BEE_F11E;
/// How far into a file the header may start.
pub const MAX_PREFIX: usize = 4096;
/// The largest image accepted.
pub const MAX_IMAGE: u32 = 16 * 1024 * 1024;

/// The OTA cluster.
pub const CLUSTER: u16 = 0x0019;

/// OTA cluster command ids.
pub mod command {
    /// Server to client: an image may be available.
    pub const IMAGE_NOTIFY: u8 = 0x00;
    /// Client to server.
    pub const QUERY_NEXT_IMAGE_REQUEST: u8 = 0x01;
    /// Server to client.
    pub const QUERY_NEXT_IMAGE_RESPONSE: u8 = 0x02;
    /// Client to server.
    pub const IMAGE_BLOCK_REQUEST: u8 = 0x03;
    /// Client to server.
    pub const IMAGE_PAGE_REQUEST: u8 = 0x04;
    /// Server to client.
    pub const IMAGE_BLOCK_RESPONSE: u8 = 0x05;
    /// Client to server.
    pub const UPGRADE_END_REQUEST: u8 = 0x06;
    /// Server to client.
    pub const UPGRADE_END_RESPONSE: u8 = 0x07;
}

/// OTA status codes.
pub mod status {
    /// Success.
    pub const SUCCESS: u8 = 0x00;
    /// Abort the download.
    pub const ABORT: u8 = 0x95;
    /// No image for this device.
    pub const NO_IMAGE_AVAILABLE: u8 = 0x98;
}

/// An OTA file header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtaHeader {
    /// Header version (0x0100).
    pub header_version: u16,
    /// Header length.
    pub header_length: u16,
    /// Field control: which optional fields follow.
    pub field_control: u16,
    /// Manufacturer code.
    pub manufacturer: u16,
    /// Image type.
    pub image_type: u16,
    /// File version.
    pub file_version: u32,
    /// Zigbee stack version.
    pub stack_version: u16,
    /// Header string (free text).
    pub header_string: String,
    /// The whole image's size, header included.
    pub total_size: u32,
    /// Only for this device, if set.
    pub destination: Option<Ieee>,
    /// Hardware versions it fits, if set.
    pub hardware: Option<(u16, u16)>,
}

/// An OTA file did not parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OtaError {
    /// No OTA header in the first [`MAX_PREFIX`] bytes.
    #[error("no OTA header")]
    NoHeader,
    /// The header is cut short.
    #[error("OTA header: {0}")]
    Truncated(#[from] Truncated),
    /// The header's sizes do not add up.
    #[error("OTA header: {0}")]
    Invalid(String),
}

impl OtaHeader {
    /// Find and parse the header in the start of a file. Returns where the
    /// image starts (the header's offset) and the header.
    pub fn find(bytes: &[u8]) -> Result<(usize, OtaHeader), OtaError> {
        let magic = MAGIC.to_le_bytes();
        let window = bytes
            .get(..bytes.len().min(MAX_PREFIX + 4))
            .unwrap_or(bytes);
        let start = window
            .windows(4)
            .position(|w| w == magic)
            .ok_or(OtaError::NoHeader)?;
        let header = OtaHeader::parse(bytes.get(start..).unwrap_or(&[]))?;
        Ok((start, header))
    }

    /// Parse a header at the start of `bytes`.
    pub fn parse(bytes: &[u8]) -> Result<OtaHeader, OtaError> {
        let mut r = Reader::new(bytes);
        if r.u32()? != MAGIC {
            return Err(OtaError::NoHeader);
        }
        let header_version = r.u16()?;
        let header_length = r.u16()?;
        let field_control = r.u16()?;
        let manufacturer = r.u16()?;
        let image_type = r.u16()?;
        let file_version = r.u32()?;
        let stack_version = r.u16()?;
        let header_string = r
            .bytes(32)?
            .iter()
            .take_while(|b| **b != 0)
            .map(|b| {
                if b.is_ascii_graphic() || *b == b' ' {
                    *b as char
                } else {
                    '?'
                }
            })
            .collect::<String>()
            .trim()
            .to_string();
        let total_size = r.u32()?;
        if field_control & 0x0001 != 0 {
            let _security_credential_version = r.u8()?;
        }
        let destination = if field_control & 0x0002 != 0 {
            Some(r.ieee()?)
        } else {
            None
        };
        let hardware = if field_control & 0x0004 != 0 {
            Some((r.u16()?, r.u16()?))
        } else {
            None
        };
        if usize::from(header_length) < r.position() {
            return Err(OtaError::Invalid(format!(
                "header length {header_length} is shorter than its fields"
            )));
        }
        if total_size < u32::from(header_length) || total_size > MAX_IMAGE {
            return Err(OtaError::Invalid(format!("image size {total_size}")));
        }
        Ok(OtaHeader {
            header_version,
            header_length,
            field_control,
            manufacturer,
            image_type,
            file_version,
            stack_version,
            header_string,
            total_size,
            destination,
            hardware,
        })
    }

    /// Whether this image is for a device with this identity.
    pub fn fits(
        &self,
        manufacturer: u16,
        image_type: u16,
        hardware: Option<u16>,
        ieee: Ieee,
    ) -> bool {
        self.manufacturer == manufacturer
            && self.image_type == image_type
            && self.destination.is_none_or(|d| d == ieee)
            && match (self.hardware, hardware) {
                (Some((min, max)), Some(hw)) => (min..=max).contains(&hw),
                _ => true,
            }
    }
}

/// One image file in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtaImage {
    /// Its header.
    pub header: OtaHeader,
    /// The file.
    pub path: PathBuf,
    /// Where the image starts in the file (after any vendor prefix).
    pub offset: u64,
}

impl OtaImage {
    /// Read an image file's header.
    pub fn open(path: &Path) -> std::io::Result<Result<OtaImage, OtaError>> {
        use std::io::Read;
        let mut head = Vec::with_capacity(MAX_PREFIX + 128);
        std::fs::File::open(path)?
            .take((MAX_PREFIX + 128) as u64)
            .read_to_end(&mut head)?;
        let len = std::fs::metadata(path)?.len();
        Ok(OtaHeader::find(&head).and_then(|(offset, header)| {
            if offset as u64 + u64::from(header.total_size) > len {
                return Err(OtaError::Invalid(format!(
                    "the file is shorter than its image size {}",
                    header.total_size
                )));
            }
            Ok(OtaImage {
                header,
                path: path.to_path_buf(),
                offset: offset as u64,
            })
        }))
    }

    /// Read `len` bytes of the image at `offset` (cut at the image's end).
    pub async fn read(&self, offset: u32, len: usize) -> std::io::Result<Vec<u8>> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        let end = self
            .header
            .total_size
            .min(offset.saturating_add(len as u32));
        let n = end.saturating_sub(offset) as usize;
        let mut file = tokio::fs::File::open(&self.path).await?;
        file.seek(std::io::SeekFrom::Start(self.offset + u64::from(offset)))
            .await?;
        let mut buf = vec![0u8; n];
        file.read_exact(&mut buf).await?;
        Ok(buf)
    }
}

/// A directory of OTA files (`*.ota`, `*.zigbee`, `*.bin`, `*.ota.signed`,
/// or anything else with an OTA header).
#[derive(Debug, Clone)]
pub struct OtaStore {
    dir: PathBuf,
}

impl OtaStore {
    /// A store over `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> OtaStore {
        OtaStore { dir: dir.into() }
    }

    /// The directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every image in the directory. Files that are not OTA images are
    /// skipped.
    pub fn images(&self) -> Vec<OtaImage> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| match OtaImage::open(&e.path()) {
                Ok(Ok(image)) => Some(image),
                Ok(Err(err)) => {
                    tracing::debug!(path = %e.path().display(), error = %err, "not an OTA image");
                    None
                }
                Err(err) => {
                    tracing::warn!(path = %e.path().display(), error = %err, "reading an OTA image");
                    None
                }
            })
            .collect()
    }

    /// The newest image for a device newer than `current`.
    pub fn newer(
        &self,
        manufacturer: u16,
        image_type: u16,
        current: u32,
        hardware: Option<u16>,
        ieee: Ieee,
    ) -> Option<OtaImage> {
        self.images()
            .into_iter()
            .filter(|i| i.header.fits(manufacturer, image_type, hardware, ieee))
            .filter(|i| i.header.file_version > current)
            .max_by_key(|i| i.header.file_version)
    }
}

/// Build an OTA file: a header for `manufacturer`, `image_type` and
/// `file_version`, then `body`. For tests and tools.
pub fn build(manufacturer: u16, image_type: u16, file_version: u32, body: &[u8]) -> Vec<u8> {
    let header_length: u16 = 56;
    let total = u32::from(header_length) + body.len() as u32;
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&0x0100u16.to_le_bytes());
    out.extend_from_slice(&header_length.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&manufacturer.to_le_bytes());
    out.extend_from_slice(&image_type.to_le_bytes());
    out.extend_from_slice(&file_version.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    let mut text = [0u8; 32];
    for (d, s) in text.iter_mut().zip(b"zigbee-bridge test image") {
        *d = *s;
    }
    out.extend_from_slice(&text);
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_header_after_a_vendor_prefix() {
        let image = build(0x1234, 7, 0x0102_0304, &[9; 100]);
        let mut file = vec![0xAA; 70];
        file.extend_from_slice(&image);
        let (offset, h) = OtaHeader::find(&file).unwrap();
        assert_eq!(offset, 70);
        assert_eq!(
            (h.manufacturer, h.image_type, h.file_version),
            (0x1234, 7, 0x0102_0304)
        );
        assert_eq!(h.total_size, 156);
        assert_eq!(h.header_string, "zigbee-bridge test image");
        assert!(h.fits(0x1234, 7, Some(3), Ieee(1)));
        assert!(!h.fits(0x1234, 8, None, Ieee(1)));
    }

    #[test]
    fn rejects_bad_headers() {
        assert_eq!(OtaHeader::find(&[0; 64]), Err(OtaError::NoHeader));
        let mut image = build(1, 1, 1, &[]);
        image[52..56].copy_from_slice(&10u32.to_le_bytes());
        assert!(matches!(
            OtaHeader::parse(&image),
            Err(OtaError::Invalid(_))
        ));
        assert!(OtaHeader::parse(&image[..20]).is_err());
    }
}
