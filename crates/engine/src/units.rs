//! Byte sizes in binary units (1 KiB is 1024 bytes), written once.
//!
//! Byte counts on disk and over the network are `u64`: sizes, limits, rates.
//! Lengths in memory (buffers, test data) are `usize`. Each unit comes in both,
//! so a size is written as it reads (`4 * MIB`) without a conversion.
//!
//! ```
//! use wirecatch_engine::units::{MIB, MIB_USIZE};
//!
//! let limit: u64 = 10 * MIB;
//! let buffer = vec![0u8; MIB_USIZE];
//! assert_eq!(u64::try_from(buffer.len()).unwrap() * 10, limit);
//! ```

/// One kibibyte, 1024 bytes.
pub const KIB: u64 = 1024;
/// One mebibyte, 1024 KiB.
pub const MIB: u64 = 1024 * KIB;
/// One gibibyte, 1024 MiB.
pub const GIB: u64 = 1024 * MIB;

/// [`KIB`] as a length in memory.
pub const KIB_USIZE: usize = 1024;
/// [`MIB`] as a length in memory.
pub const MIB_USIZE: usize = 1024 * KIB_USIZE;
