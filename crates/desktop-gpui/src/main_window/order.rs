//! The order of downloads in a queue's line: where Alt+Up and Alt+Down move the selection.

use wirecatch_protocol::DownloadId;

/// `line` with the `chosen` downloads moved one place up (`up`) or down, keeping their own order;
/// `None` when they are already at that end or none of the line is chosen.
pub fn nudged(line: &[DownloadId], chosen: &[DownloadId], up: bool) -> Option<Vec<DownloadId>> {
    let moving: Vec<DownloadId> = line
        .iter()
        .copied()
        .filter(|id| chosen.contains(id))
        .collect();
    let first = line.iter().position(|id| chosen.contains(id))?;
    let last = line.iter().rposition(|id| chosen.contains(id))?;
    let rest: Vec<DownloadId> = line
        .iter()
        .copied()
        .filter(|id| !chosen.contains(id))
        .collect();
    let at = if up {
        // The one just above the first chosen is never chosen itself.
        let above = *line.get(first.checked_sub(1)?)?;
        rest.iter().position(|id| *id == above)?
    } else {
        let below = *line.get(last + 1)?;
        rest.iter().position(|id| *id == below)? + 1
    };
    let mut next = rest;
    next.splice(at..at, moving);
    (next != line).then_some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[i64]) -> Vec<DownloadId> {
        list.iter().copied().map(DownloadId).collect()
    }

    #[test]
    fn the_chosen_move_one_place_together() {
        let line = ids(&[1, 2, 3, 4, 5]);
        assert_eq!(nudged(&line, &ids(&[3]), true), Some(ids(&[1, 3, 2, 4, 5])));
        assert_eq!(
            nudged(&line, &ids(&[3]), false),
            Some(ids(&[1, 2, 4, 3, 5]))
        );
        assert_eq!(
            nudged(&line, &ids(&[2, 4]), true),
            Some(ids(&[2, 4, 1, 3, 5]))
        );
        assert_eq!(
            nudged(&line, &ids(&[2, 4]), false),
            Some(ids(&[1, 3, 5, 2, 4]))
        );
    }

    #[test]
    fn nothing_moves_past_an_end_or_without_a_choice() {
        let line = ids(&[1, 2, 3]);
        assert_eq!(nudged(&line, &ids(&[1]), true), None);
        assert_eq!(nudged(&line, &ids(&[3]), false), None);
        assert_eq!(nudged(&line, &ids(&[9]), true), None);
    }
}
