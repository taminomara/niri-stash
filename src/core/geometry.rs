//! Edge choice, the push toward an edge, and the nudge back. Pure.
//!
//! Positions are output-relative logical pixels as niri reports them
//! (`tile_pos_in_workspace_view`); sizes are the window size niri reports (`window_size`).
//! Every move the daemon sends is a relative nudge. The daemon never resizes: niri keeps a
//! quarter of each dimension visible, clamped to 10–75 px, so a pushed window shows a strip of up
//! to 75 px.

use serde::{Deserialize, Serialize};

/// How far past the edge a stash nudges; niri clamps it.
const PUSH_DISTANCE_PX: f64 = 10_000.0;
/// Two geometries within this many logical pixels on every value are the same.
const GEOMETRY_TOLERANCE_PX: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Geometry {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) w: f64,
    pub(crate) h: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// A relative move: how far to nudge on each axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Nudge {
    pub(crate) dx: f64,
    pub(crate) dy: f64,
}

/// The output edge closest to the window's centre, measured from the output's logical edges.
/// Ties resolve left, right, top, bottom (spec D3).
pub(crate) fn nearest_output_edge(win: Geometry, output_w: f64, output_h: f64) -> Edge {
    let cx = win.x + win.w / 2.0;
    let cy = win.y + win.h / 2.0;
    let candidates = [
        (Edge::Left, cx),
        (Edge::Right, output_w - cx),
        (Edge::Top, cy),
        (Edge::Bottom, output_h - cy),
    ];
    let (mut nearest, mut nearest_distance) = candidates[0];
    for (edge, distance) in candidates {
        if distance < nearest_distance {
            (nearest, nearest_distance) = (edge, distance);
        }
    }
    nearest
}

/// The nudge that pushes a window toward `edge` until niri stops it.
pub(crate) fn push_toward(edge: Edge) -> Nudge {
    match edge {
        Edge::Left => Nudge {
            dx: -PUSH_DISTANCE_PX,
            dy: 0.0,
        },
        Edge::Right => Nudge {
            dx: PUSH_DISTANCE_PX,
            dy: 0.0,
        },
        Edge::Top => Nudge {
            dx: 0.0,
            dy: -PUSH_DISTANCE_PX,
        },
        Edge::Bottom => Nudge {
            dx: 0.0,
            dy: PUSH_DISTANCE_PX,
        },
    }
}

/// The nudge that moves a window from `from` to `to`, or `None` when it is already there within
/// tolerance.
pub(crate) fn nudge_between(from: Geometry, to: Geometry) -> Option<Nudge> {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    if dx.abs() <= GEOMETRY_TOLERANCE_PX && dy.abs() <= GEOMETRY_TOLERANCE_PX {
        None
    } else {
        Some(Nudge { dx, dy })
    }
}

/// True when every value of `a` and `b` is within `GEOMETRY_TOLERANCE_PX`.
pub(crate) fn within_tolerance(a: Geometry, b: Geometry) -> bool {
    [a.x - b.x, a.y - b.y, a.w - b.w, a.h - b.h]
        .iter()
        .all(|d| d.abs() <= GEOMETRY_TOLERANCE_PX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT_W: f64 = 1828.0;
    const OUT_H: f64 = 1142.0;

    fn geo(x: f64, y: f64, w: f64, h: f64) -> Geometry {
        Geometry { x, y, w, h }
    }

    #[test]
    fn picks_left_edge_when_centre_is_closest() {
        // centre (150, 571): left 150, right 1678, top 571, bottom 571
        assert_eq!(
            nearest_output_edge(geo(50.0, 471.0, 200.0, 200.0), OUT_W, OUT_H),
            Edge::Left
        );
    }

    #[test]
    fn picks_right_edge_when_centre_is_closest() {
        // centre (1678, 571): left 1678, right 150, top 571, bottom 571
        assert_eq!(
            nearest_output_edge(geo(1578.0, 471.0, 200.0, 200.0), OUT_W, OUT_H),
            Edge::Right
        );
    }

    #[test]
    fn picks_top_edge_when_centre_is_closest() {
        // centre (914, 150): left 914, right 914, top 150, bottom 992
        assert_eq!(
            nearest_output_edge(geo(814.0, 50.0, 200.0, 200.0), OUT_W, OUT_H),
            Edge::Top
        );
    }

    #[test]
    fn picks_bottom_edge_when_centre_is_closest() {
        // centre (914, 992): left 914, right 914, top 992, bottom 150
        assert_eq!(
            nearest_output_edge(geo(814.0, 892.0, 200.0, 200.0), OUT_W, OUT_H),
            Edge::Bottom
        );
    }

    #[test]
    fn horizontal_tie_goes_left() {
        // centre (914, 571): left 914, right 914, top 571, bottom 571; output is wider than tall,
        // so make the vertical distances larger by using a tall output.
        assert_eq!(
            nearest_output_edge(geo(814.0, 1900.0, 200.0, 200.0), OUT_W, 4000.0),
            Edge::Left
        );
    }

    #[test]
    fn four_way_tie_goes_left() {
        // square output, centred window: every distance is 400
        assert_eq!(
            nearest_output_edge(geo(300.0, 300.0, 200.0, 200.0), 800.0, 800.0),
            Edge::Left
        );
    }

    #[test]
    fn geometries_within_one_pixel_on_every_value_are_the_same_and_beyond_it_are_not() {
        let a = geo(100.0, 200.0, 800.0, 600.0);
        assert!(within_tolerance(a, a));
        assert!(within_tolerance(a, geo(101.0, 199.0, 801.0, 599.0)));
        assert!(!within_tolerance(a, geo(101.5, 200.0, 800.0, 600.0)));
        assert!(!within_tolerance(a, geo(100.0, 198.0, 800.0, 600.0)));
        assert!(!within_tolerance(a, geo(100.0, 200.0, 802.0, 600.0)));
        assert!(!within_tolerance(a, geo(100.0, 200.0, 800.0, 597.0)));
    }

    #[test]
    fn pushing_toward_an_edge_nudges_far_past_it_on_that_axis_only() {
        assert_eq!(
            push_toward(Edge::Left),
            Nudge {
                dx: -10000.0,
                dy: 0.0
            }
        );
        assert_eq!(
            push_toward(Edge::Right),
            Nudge {
                dx: 10000.0,
                dy: 0.0
            }
        );
        assert_eq!(
            push_toward(Edge::Top),
            Nudge {
                dx: 0.0,
                dy: -10000.0
            }
        );
        assert_eq!(
            push_toward(Edge::Bottom),
            Nudge {
                dx: 0.0,
                dy: 10000.0
            }
        );
    }

    #[test]
    fn the_nudge_between_two_positions_is_their_difference_and_none_when_already_there() {
        let from = geo(-525.0, 200.0, 600.0, 300.0);
        let to = geo(100.0, 250.0, 600.0, 300.0);
        assert_eq!(
            nudge_between(from, to),
            Some(Nudge {
                dx: 625.0,
                dy: 50.0
            })
        );
        assert_eq!(nudge_between(to, geo(100.5, 249.5, 600.0, 300.0)), None);
    }
}
