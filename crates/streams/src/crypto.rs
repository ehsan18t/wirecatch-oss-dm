//! HLS `METHOD=AES-128`: AES-128 in CBC mode with PKCS#7 padding, one whole
//! segment at a time.

use aes::Aes128;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockDecryptMut, KeyIvInit};

/// Decrypts `data` in place and returns the length of the plain text, or
/// `None` when the padding is wrong (the key or IV does not fit, or the data
/// is not a whole number of blocks).
pub(crate) fn decrypt(data: &mut [u8], key: &[u8; 16], iv: &[u8; 16]) -> Option<usize> {
    let decryptor = cbc::Decryptor::<Aes128>::new_from_slices(key, iv).ok()?;
    decryptor
        .decrypt_padded_mut::<Pkcs7>(data)
        .ok()
        .map(|plain| plain.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::cipher::BlockEncryptMut;

    fn encrypt(plain: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
        cbc::Encryptor::<Aes128>::new_from_slices(key, iv)
            .unwrap()
            .encrypt_padded_vec_mut::<Pkcs7>(plain)
    }

    #[test]
    fn round_trips_and_detects_a_wrong_key() {
        let key = [7u8; 16];
        let iv = [9u8; 16];
        for len in [0, 1, 15, 16, 17, 4096] {
            let plain: Vec<u8> = (0..len).map(|i| (i * 31) as u8).collect();
            let mut data = encrypt(&plain, &key, &iv);
            assert_eq!(data.len() % 16, 0);
            let n = decrypt(&mut data, &key, &iv).unwrap();
            assert_eq!(&data[..n], &plain[..]);
        }
        let mut data = encrypt(&[1; 100], &key, &iv);
        // The wrong key gives wrong padding (with overwhelming likelihood).
        assert_eq!(decrypt(&mut data, &[8; 16], &iv), None);
        let mut ragged = vec![0u8; 17];
        assert_eq!(decrypt(&mut ragged, &key, &iv), None);
    }

    #[test]
    fn known_answer() {
        // NIST SP 800-38A, F.2.1 CBC-AES128, first block, plus one block of
        // PKCS#7 padding encrypted by the same key.
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
        let mut data = encrypt(&plain, &key, &iv);
        assert_eq!(&data[..16], &expected_first);
        let n = decrypt(&mut data, &key, &iv).unwrap();
        assert_eq!(&data[..n], &plain);
    }
}
