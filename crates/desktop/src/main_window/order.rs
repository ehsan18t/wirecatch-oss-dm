//! The order of downloads in a queue's line, and of the queues: where Alt+Up and Alt+Down move
//! the selection, and where dragged rows and queues land.

use wirecatch_protocol::DownloadId;

/// Puts queue `queue`'s line in `order`: the manager first, then the list, which no event tells.
pub fn apply(
    library: &gpui_kit::Entity<crate::library::Library>,
    queue: wirecatch_protocol::QueueId,
    order: Vec<DownloadId>,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) {
    use gpui_kit::component::WindowExt as _;

    let sent = order.clone();
    let done = crate::backend::Backend::run(cx, move |manager| async move {
        wirecatch_shell::api::reorder(&manager, queue, &sent)
    });
    let library = library.clone();
    window
        .spawn(cx, async move |cx| {
            let refused = match done.await {
                Ok(Ok(Ok(()))) => None,
                Ok(Ok(Err(refused))) => Some(refused.message),
                Ok(Err(unopened)) => Some(unopened.reason.message),
                Err(_) => return,
            };
            _ = cx.update(|window, cx| match refused {
                None => library.update(cx, |library, cx| {
                    library.reorder(&order);
                    cx.notify();
                }),
                Some(message) => window.push_notification(message, cx),
            });
        })
        .detach();
}

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

/// `line` with the `moving` items put just before `before`, or at the end when it is `None` or
/// not in the line, keeping their own order; `None` when the order would not change.
pub fn placed<T: Copy + PartialEq>(line: &[T], moving: &[T], before: Option<T>) -> Option<Vec<T>> {
    let moved: Vec<T> = line
        .iter()
        .copied()
        .filter(|id| moving.contains(id))
        .collect();
    if moved.is_empty() {
        return None;
    }
    let mut next: Vec<T> = line
        .iter()
        .copied()
        .filter(|id| !moving.contains(id))
        .collect();
    let at = before
        .and_then(|before| next.iter().position(|id| *id == before))
        .unwrap_or(next.len());
    next.splice(at..at, moved);
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

    #[test]
    fn dropped_items_land_before_their_target_in_their_own_order() {
        let line = ids(&[1, 2, 3, 4, 5]);
        assert_eq!(
            placed(&line, &ids(&[4, 2]), Some(DownloadId(1))),
            Some(ids(&[2, 4, 1, 3, 5]))
        );
        assert_eq!(placed(&line, &ids(&[1]), None), Some(ids(&[2, 3, 4, 5, 1])));
        assert_eq!(
            placed(&line, &ids(&[2]), Some(DownloadId(3))),
            None,
            "no change"
        );
        assert_eq!(placed(&line, &ids(&[9]), None), None);
    }
}
