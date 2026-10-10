//! HLS `METHOD=AES-128`: AES-128 in CBC mode with PKCS#7 padding.
//!
//! Segments are decrypted as a stream, a buffer at a time, so memory use does
//! not grow with the segment's size. The last block is held back until the
//! end, because it carries the padding.

use std::io::{self, Read, Write};

use aes::Aes128;
use cbc::cipher::generic_array::GenericArray;
use cbc::cipher::{BlockDecryptMut, KeyIvInit};

const BLOCK: usize = 16;

/// Bytes decrypted per step.
const CHUNK: usize = 256 * 1024;

/// Decrypts `input` into `output` and returns the length of the plain text,
/// or `None` when the data does not decrypt (the padding is wrong, so the key
/// or IV does not fit, or the data is not a whole number of blocks).
pub(crate) fn decrypt(
    input: &mut impl Read,
    output: &mut impl Write,
    key: &[u8; 16],
    iv: &[u8; 16],
) -> io::Result<Option<u64>> {
    decrypt_in_chunks(input, output, key, iv, CHUNK)
}

fn decrypt_in_chunks(
    input: &mut impl Read,
    output: &mut impl Write,
    key: &[u8; 16],
    iv: &[u8; 16],
    chunk: usize,
) -> io::Result<Option<u64>> {
    debug_assert!(chunk > 0 && chunk % BLOCK == 0);
    let mut cipher = cbc::Decryptor::<Aes128>::new(key.into(), iv.into());
    // One chunk plus the block held back from the previous one.
    let mut buf = vec![0u8; chunk + BLOCK];
    let mut held = 0;
    let mut written = 0u64;
    loop {
        let filled = fill(input, &mut buf[held..])?;
        held += filled;
        if held < buf.len() {
            // The end: everything left, padding included.
            if held == 0 || held % BLOCK != 0 {
                return Ok(None);
            }
            decrypt_blocks(&mut cipher, &mut buf[..held]);
            let Some(plain) = unpad(&buf[..held]) else {
                return Ok(None);
            };
            output.write_all(&buf[..plain])?;
            return Ok(Some(written + plain as u64));
        }
        // A full buffer: decrypt and write all but its last block, which may
        // be the final one with the padding.
        let ready = held - BLOCK;
        decrypt_blocks(&mut cipher, &mut buf[..ready]);
        output.write_all(&buf[..ready])?;
        written += ready as u64;
        buf.copy_within(ready..held, 0);
        held = BLOCK;
    }
}

/// Reads until `buf` is full or the input ends; returns how much was read.
fn fill(input: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match input.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Decrypts whole blocks in place; `data.len()` is a multiple of 16.
fn decrypt_blocks(cipher: &mut cbc::Decryptor<Aes128>, data: &mut [u8]) {
    for block in data.chunks_exact_mut(BLOCK) {
        cipher.decrypt_block_mut(GenericArray::from_mut_slice(block));
    }
}

/// The length without PKCS#7 padding, or `None` if the padding is invalid.
fn unpad(data: &[u8]) -> Option<usize> {
    let pad = usize::from(*data.last()?);
    if pad == 0 || pad > BLOCK || pad > data.len() {
        return None;
    }
    let start = data.len() - pad;
    data[start..]
        .iter()
        .all(|b| usize::from(*b) == pad)
        .then_some(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::cipher::BlockEncryptMut;
    use cbc::cipher::block_padding::Pkcs7;

    fn encrypt(plain: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
        cbc::Encryptor::<Aes128>::new_from_slices(key, iv)
            .unwrap()
            .encrypt_padded_vec_mut::<Pkcs7>(plain)
    }

    fn run(data: &[u8], key: &[u8; 16], iv: &[u8; 16], chunk: usize) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let n = decrypt_in_chunks(&mut &data[..], &mut out, key, iv, chunk).unwrap()?;
        assert_eq!(n, out.len() as u64);
        Some(out)
    }

    #[test]
    fn round_trips_across_chunk_boundaries() {
        let key = [7u8; 16];
        let iv = [9u8; 16];
        // Lengths around the chunk size, so the held-back block and the
        // padding land on every side of a chunk boundary.
        for len in [0, 1, 15, 16, 17, 31, 32, 33, 47, 48, 49, 64, 100, 4096] {
            let plain: Vec<u8> = (0..len).map(|i| (i * 31 + 7) as u8).collect();
            let data = encrypt(&plain, &key, &iv);
            for chunk in [16, 32, 48, CHUNK] {
                assert_eq!(
                    run(&data, &key, &iv, chunk).unwrap(),
                    plain,
                    "len {len}, chunk {chunk}"
                );
            }
        }
    }

    #[test]
    fn a_wrong_key_or_ragged_data_is_detected() {
        let key = [7u8; 16];
        let iv = [9u8; 16];
        let data = encrypt(&[1; 100], &key, &iv);
        // The wrong key gives wrong padding (with overwhelming likelihood).
        assert_eq!(run(&data, &[8; 16], &iv, 32), None);
        assert_eq!(run(&data[..data.len() - 1], &key, &iv, 32), None);
        assert_eq!(run(&[], &key, &iv, 32), None);
    }

    #[test]
    fn known_answer() {
        // NIST SP 800-38A, F.2.1 CBC-AES128, first block.
        let key: [u8; 16] = [
            0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf,
            0x4f, 0x3c,
        ];
        let iv: [u8; 16] = core::array::from_fn(|i| i as u8);
        let plain: [u8; 16] = [
            0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93,
            0x17, 0x2a,
        ];
        let expected_first: [u8; 16] = [
            0x76, 0x49, 0xab, 0xac, 0x81, 0x19, 0xb2, 0x46, 0xce, 0xe9, 0x8e, 0x9b, 0x12, 0xe9,
            0x19, 0x7d,
        ];
        let data = encrypt(&plain, &key, &iv);
        assert_eq!(&data[..16], &expected_first);
        assert_eq!(run(&data, &key, &iv, 16).unwrap(), plain);
    }

    #[test]
    fn padding_rules() {
        assert_eq!(unpad(&[1, 2, 3, 1]), Some(3));
        assert_eq!(unpad(&[4, 4, 4, 4]), Some(0));
        assert_eq!(unpad(&[1, 2, 3, 0]), None);
        assert_eq!(unpad(&[1, 2, 3, 3]), None);
        assert_eq!(unpad(&[17; 17]), None);
        assert_eq!(unpad(&[]), None);
    }
}
