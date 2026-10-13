//! `SegmentList`: its entries, read once on the level they are on, and resolved into a representation's pieces only for the representations chosen.

#![forbid(unsafe_code)]

use std::ops::Range;
use std::sync::Arc;

use roxmltree::Node;
use url::Url;

use super::{byte_range, children, join};
use crate::error::ManifestError;
use crate::plan::{AddressBudget, Piece, check_room};

/// What one `SegmentURL` or list `Initialization` says: an address relative to the representation's base (the base itself when it has none), and a byte range.
#[derive(Debug)]
pub(super) struct Entry {
    address: Option<Box<str>>,
    range: Option<Range<u64>>,
}

impl Entry {
    /// The entry `node` gives through its attributes `address` and `range`. A byte range that cannot be read is refused here; an address only when it is resolved.
    pub(super) fn read(
        node: Node<'_, '_>,
        address: &str,
        range: &str,
    ) -> Result<Self, ManifestError> {
        Ok(Entry {
            address: node.attribute(address).map(Box::from),
            range: node.attribute(range).map(byte_range).transpose()?,
        })
    }

    /// The `SegmentURL` entries of the `SegmentList` `list`.
    pub(super) fn segments(list: Node<'_, '_>) -> Result<Arc<[Entry]>, ManifestError> {
        children(list, "SegmentURL")
            .map(|segment| Entry::read(segment, "media", "mediaRange"))
            .collect()
    }

    fn piece(&self, base: &Url, budget: &mut AddressBudget) -> Result<Piece, ManifestError> {
        let url = match &self.address {
            Some(address) => join(base, address)?,
            None => base.clone(),
        };
        budget.spend(&url)?;
        Ok(Piece::plain(url, self.range.clone()))
    }
}

/// A representation's `SegmentList`, shared with the other representations of the level it is on, and its base address.
#[derive(Debug)]
pub(crate) struct ListSegments {
    base: Arc<Url>,
    init: Option<Arc<Entry>>,
    segments: Arc<[Entry]>,
}

impl ListSegments {
    pub(super) fn new(base: Arc<Url>, init: Option<Arc<Entry>>, segments: Arc<[Entry]>) -> Self {
        ListSegments {
            base,
            init,
            segments,
        }
    }

    /// The pieces to fetch, init section first, each address counted against the track's budget.
    pub fn expand(&self) -> Result<Vec<Piece>, ManifestError> {
        let mut pieces = Vec::new();
        let mut budget = AddressBudget::default();
        if let Some(init) = &self.init {
            let mut piece = init.piece(&self.base, &mut budget)?;
            piece.init = true;
            pieces.push(piece);
        }
        for segment in self.segments.iter() {
            check_room(&pieces)?;
            pieces.push(segment.piece(&self.base, &mut budget)?);
        }
        Ok(pieces)
    }
}
