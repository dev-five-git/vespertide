//! Rank assignment + grid layout for the SVG ERD renderer.
//!
//! Tables are placed in topological ranks (parents to the left, children
//! to the right). Lopsided layouts are rebalanced into a roughly square
//! grid so the resulting diagram is easier to read.

// Layout maps integer rank / column counts into floating-point pixel
// coordinates; the casts are bounded by the table count and the diagonal
// rebalance heuristic.
#![expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "SVG layout converts bounded table/row counts into pixel coordinates"
)]
#![expect(
    clippy::range_plus_one,
    reason = "SVG row/rank math mirrors visual inclusive ranges in the renderer"
)]

use super::model::TableBox;
use super::style::{NODE_GAP, RANK_GAP, VIEW_PAD};

use super::model::EdgeSpec;

pub(super) fn compute_ranks(boxes: &[TableBox], edges: &[EdgeSpec]) -> Vec<usize> {
    let n = boxes.len();
    let mut parents: Vec<Vec<usize>> = vec![Vec::new(); n];
    for edge in edges {
        parents[edge.child_idx].push(edge.parent_idx);
    }

    let mut ranks = vec![0_usize; n];
    // Iterative fixed-point; cap iterations to avoid cycles spiralling.
    for _ in 0..(n + 1) {
        let mut changed = false;
        for i in 0..n {
            let candidate = parents[i]
                .iter()
                .map(|&p| ranks[p].saturating_add(1))
                .max()
                .unwrap_or(0);
            if candidate > ranks[i] {
                ranks[i] = candidate;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    ranks
}

pub(super) fn layout_grid(boxes: &mut [TableBox], ranks: &[usize]) {
    let max_rank = ranks.iter().copied().max().unwrap_or(0);
    let num_ranks = max_rank + 1;

    // Bucket by rank.
    let mut groups: Vec<Vec<usize>> = vec![Vec::new(); num_ranks];
    for (i, &r) in ranks.iter().enumerate() {
        groups[r].push(i);
    }

    // Stable order inside each rank: by name.
    for group in &mut groups {
        group.sort_by(|&a, &b| boxes[a].name.cmp(&boxes[b].name));
    }

    // If the layout is very lopsided (one rank stuffed full while another is sparse),
    // rebalance by splitting the largest rank.
    rebalance_groups(&mut groups, boxes.len());

    // Compute per-rank column width as max box width.
    let col_widths: Vec<f64> = groups
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|&i| boxes[i].width)
                .fold(180.0_f64, f64::max)
        })
        .collect();

    // X positions per rank (left edge of column).
    let mut col_x = Vec::with_capacity(groups.len());
    let mut cursor = VIEW_PAD;
    for w in &col_widths {
        col_x.push(cursor);
        cursor += *w + RANK_GAP;
    }

    // Place inside each column, centered horizontally on the column's width.
    for (rank_idx, group) in groups.iter().enumerate() {
        let mut y = VIEW_PAD;
        let column_x = col_x[rank_idx];
        let column_w = col_widths[rank_idx];
        for &i in group {
            let bx = &mut boxes[i];
            bx.x = column_x + (column_w - bx.width) / 2.0;
            bx.y = y;
            y += bx.height + NODE_GAP;
        }
    }
}

fn rebalance_groups(groups: &mut Vec<Vec<usize>>, total: usize) {
    if groups.is_empty() {
        return;
    }
    let target_max = ((total as f64).sqrt().ceil() as usize).max(3);

    let mut i = 0;
    while i < groups.len() {
        if groups[i].len() > target_max {
            let overflow: Vec<usize> = groups[i].split_off(target_max);
            groups.insert(i + 1, overflow);
        }
        i += 1;
    }
}

pub(super) fn view_size(boxes: &[TableBox]) -> (f64, f64) {
    let mut w = 0.0_f64;
    let mut h = 0.0_f64;
    for bx in boxes {
        w = w.max(bx.x + bx.width);
        h = h.max(bx.y + bx.height);
    }
    (w + VIEW_PAD, h + VIEW_PAD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebalance_groups_empty_is_noop() {
        let mut groups = Vec::new();
        rebalance_groups(&mut groups, 0);
        assert!(groups.is_empty());
    }

    // A group with EXACTLY target_max members must NOT be split. With total=4,
    // target_max = max(ceil(sqrt(4)), 3) = 3, so a 3-element group is at the
    // boundary. Pins `groups[i].len() > target_max`: a `>=` mutant would split
    // it, inserting a spurious (empty) overflow group.
    #[test]
    fn rebalance_groups_does_not_split_at_exactly_target_max() {
        let mut groups = vec![vec![0_usize, 1, 2]];
        rebalance_groups(&mut groups, 4);
        assert_eq!(
            groups.len(),
            1,
            "group of exactly target_max must not split"
        );
        assert_eq!(groups[0], vec![0, 1, 2]);
    }
}
