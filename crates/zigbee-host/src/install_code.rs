//! Zigbee 3.0 install codes and the AES-MMO hash that turns one into a link
//! key (Zigbee specification B.6; Base Device Behavior 10.1).

use aes::cipher::{Array, BlockCipherEncrypt, KeyInit};
use aes::Aes128;
use crc::{Crc, CRC_16_IBM_SDLC};
use zigbee_primitives::Key;

/// CRC-16/X-25, the install code checksum.
const CRC: Crc<u16> = Crc::<u16>::new(&CRC_16_IBM_SDLC);

/// An install code was not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstallCodeError {
    /// Not 6, 8, 12 or 16 bytes plus a 2-byte CRC.
    #[error("install code of {0} bytes: expected 8, 10, 14 or 18 including the CRC")]
    Length(usize),
    /// The CRC did not match.
    #[error("install code CRC mismatch")]
    Crc,
    /// Not hex.
    #[error("install code is not hex")]
    Hex,
}

/// Parse an install code written as hex (spaces and dashes ignored).
pub fn parse_hex(text: &str) -> Result<Vec<u8>, InstallCodeError> {
    let digits: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != ':')
        .collect();
    if !digits.len().is_multiple_of(2) {
        return Err(InstallCodeError::Hex);
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| {
            digits
                .get(i..i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or(InstallCodeError::Hex)
        })
        .collect()
}

/// Check an install code (with its trailing little-endian CRC) and derive
/// the link key a device joins with.
pub fn link_key(code_with_crc: &[u8]) -> Result<Key, InstallCodeError> {
    if !matches!(code_with_crc.len(), 8 | 10 | 14 | 18) {
        return Err(InstallCodeError::Length(code_with_crc.len()));
    }
    let (code, crc) = code_with_crc.split_at(code_with_crc.len() - 2);
    let wanted = match crc {
        [lo, hi] => u16::from_le_bytes([*lo, *hi]),
        _ => return Err(InstallCodeError::Length(code_with_crc.len())),
    };
    if CRC.checksum(code) != wanted {
        return Err(InstallCodeError::Crc);
    }
    Ok(Key(aes_mmo_hash(code_with_crc)))
}

/// The Matyas-Meyer-Oseas hash with AES-128, padded as the Zigbee
/// specification (B.6) requires for messages shorter than 2^16 bits.
pub fn aes_mmo_hash(message: &[u8]) -> [u8; 16] {
    let bit_len = (message.len() as u64 * 8) as u16;
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 16 != 14 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let mut hash = [0u8; 16];
    for chunk in padded.as_chunks::<16>().0 {
        let cipher = Aes128::new(&Array::from(hash));
        let block = *chunk;
        let mut out = Array::from(block);
        cipher.encrypt_block(&mut out);
        for (h, (o, m)) in hash.iter_mut().zip(out.iter().zip(block.iter())) {
            *h = o ^ m;
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_the_published_test_vector() {
        // Zigbee Alliance / Silicon Labs AN1089 example.
        let code = parse_hex("83FED3407A939723A5C639B26916D505C3B5").unwrap();
        let key = link_key(&code).unwrap();
        assert_eq!(key.to_hex(), "66b6900981e1ee3ca4206b6b861c02bb");
    }

    #[test]
    fn rejects_bad_codes() {
        let mut code = parse_hex("83FED3407A939723A5C639B26916D505C3B5").unwrap();
        code[0] ^= 1;
        assert_eq!(link_key(&code), Err(InstallCodeError::Crc));
        assert_eq!(link_key(&[1, 2, 3]), Err(InstallCodeError::Length(3)));
        assert_eq!(parse_hex("zz"), Err(InstallCodeError::Hex));
    }
}
