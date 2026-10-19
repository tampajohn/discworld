// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 WorkOutDestination, before path routing and node projection.
use crate::{
    iso::Error,
    polygon::{Kind, Polygons},
};

struct Resolver<'a> {
    polygons: &'a Polygons,
    origin: [i32; 2],
    size: [i32; 2],
    budget: usize,
}
impl Resolver<'_> {
    fn path(&self, p: [i32; 2]) -> bool {
        self.polygons.active.iter().any(|a| {
            a.enabled && matches!(a.definition.kind, Kind::Path | Kind::NodePath) && a.contains(p)
        })
    }
    fn blocked(&self, p: [i32; 2]) -> bool {
        self.polygons.at(p, Kind::Block).is_some()
    }
    fn spend(&mut self) -> Result<(), Error> {
        self.budget = self
            .budget
            .checked_sub(1)
            .ok_or(Error("destination search budget exhausted"))?;
        Ok(())
    }
    fn on_path(&mut self, p: [i32; 2]) -> Result<Option<[i32; 2]>, Error> {
        if !self.blocked(p) {
            return Ok(Some(p));
        }
        let mut y = i64::from(p[1]) + 1;
        let end = i64::from(self.origin[1]) + 200;
        while y < end {
            self.spend()?;
            let q = [
                p[0],
                i32::try_from(y).map_err(|_| Error("destination coordinate overflow"))?,
            ];
            if !self.path(q) {
                y = 200;
                break;
            }
            if !self.blocked(q) {
                return Ok(Some(q));
            }
            y += 1;
        }
        // Preserve the original absolute SCREEN_HEIGHT comparison even when
        // the playfield is scrolled. Other fall-through cases leave the C++
        // output uninitialized; report them rather than invent a destination.
        if y != 200 {
            return Err(Error("undefined V1 blocked destination fall-through"));
        }
        y = i64::from(p[1]) - 1;
        while y >= i64::from(self.origin[1]) {
            self.spend()?;
            let q = [
                p[0],
                i32::try_from(y).map_err(|_| Error("destination coordinate overflow"))?,
            ];
            if !self.path(q) {
                return Ok(None);
            }
            if !self.blocked(q) {
                return Ok(Some(q));
            }
            y -= 1;
        }
        if y < 0 {
            Ok(None)
        } else {
            Err(Error("undefined V1 blocked destination fall-through"))
        }
    }
    fn scan(
        &mut self,
        p: [i32; 2],
        axis: usize,
        step: i64,
        end: i64,
        referral: bool,
    ) -> Result<Option<[i32; 2]>, Error> {
        let mut v = i64::from(p[axis]) + step;
        while if step > 0 { v < end } else { v >= end } {
            self.spend()?;
            let mut q = p;
            q[axis] = i32::try_from(v).map_err(|_| Error("destination coordinate overflow"))?;
            if self.path(q) {
                if !referral {
                    return self.on_path(q);
                }
                if !self.blocked(q) {
                    return Ok((!q.contains(&-1)).then_some(q));
                }
            }
            v += step;
        }
        Ok(None)
    }
}

/// Returns None when the reference completes a walk without finding a target.
/// Scene dimensions and origin are world coordinates; path membership includes
/// node paths, whose projection/routing is a separate operation.
pub fn resolve(
    polygons: &Polygons,
    point: [i32; 2],
    origin: [i32; 2],
    size: [i32; 2],
) -> Result<Option<[i32; 2]>, Error> {
    if size.iter().any(|v| *v < 0) {
        return Err(Error("negative destination scene size"));
    }
    let mut r = Resolver {
        polygons,
        origin,
        size,
        budget: 262144,
    };
    if r.path(point) {
        return r.on_path(point);
    }
    let referral = polygons
        .at(point, Kind::Refer)
        .map(|i| &polygons.active[i].definition);
    let kind = referral.map_or(polygons.default_refer, |p| p.refer_type);
    if let Some(p) = referral {
        if kind == 5 {
            return if r.path(p.node_position) && !r.blocked(p.node_position) {
                Ok((!p.node_position.contains(&-1)).then_some(p.node_position))
            } else {
                Err(Error("invalid point referral destination"))
            };
        }
    }
    let explicit = referral.is_some();
    match kind {
        1 => r.scan(point, 1, -1, 0, explicit),
        2 => r.scan(point, 1, 1, i64::from(r.size[1]), explicit),
        3 => r.scan(point, 0, -1, 0, explicit),
        4 => r.scan(point, 0, 1, i64::from(r.size[0]), explicit),
        0 if !explicit => {
            // Default referral tries the visible vertical span before the rest
            // of the background, always down before up.
            for (step, end) in [
                (1, i64::from(origin[1]) + 200),
                (-1, i64::from(origin[1])),
                (1, i64::from(size[1])),
                (-1, 0),
            ] {
                // A path hit terminates the search even if blocked adjustment
                // cannot find a destination. Track that separately from no hit.
                let mut v = i64::from(point[1]) + step;
                while if step > 0 { v < end } else { v >= end } {
                    r.spend()?;
                    let q = [
                        point[0],
                        i32::try_from(v).map_err(|_| Error("destination coordinate overflow"))?,
                    ];
                    if r.path(q) {
                        return r.on_path(q);
                    }
                    v += step;
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{graphics::Handle, polygon::Polygon};
    fn rect(kind: Kind, x: i32, y: i32, right: i32, bottom: i32) -> Polygon {
        Polygon {
            kind,
            corners: [[x, y], [right, y], [right, bottom], [x, bottom]],
            tag_position: [0; 2],
            tag_text: Handle(0),
            node_position: [0; 2],
            film: Handle(0),
            refer_type: 0,
            id: 0,
            scales: [1, 0],
            reel: 0,
            z_factor: 0,
            nodes: vec![],
            lines: vec![],
            script: Handle(0),
        }
    }
    #[test]
    fn visible_search_precedes_offscreen_and_includes_node_paths() {
        let p = Polygons::new(
            vec![
                rect(Kind::Path, 0, 250, 20, 260),
                rect(Kind::Path, 0, 10, 20, 20),
            ],
            0,
        );
        assert_eq!(
            resolve(&p, [10, 100], [0, 0], [320, 400]).unwrap(),
            Some([10, 20])
        );
        let mut p = Polygons::new(vec![rect(Kind::NodePath, 0, 250, 20, 260)], 0);
        assert_eq!(
            resolve(&p, [10, 100], [0, 0], [320, 400]).unwrap(),
            Some([10, 250])
        );
        assert_eq!(
            resolve(&p, [10, 255], [0, 0], [320, 400]).unwrap(),
            Some([10, 255])
        );
        p.active[0].enabled = false;
        assert_eq!(resolve(&p, [10, 100], [0, 0], [320, 400]).unwrap(), None);
    }
    #[test]
    fn referral_directions_point_and_path_priority() {
        for (kind, target) in [(1, [50, 30]), (2, [50, 70]), (3, [30, 50]), (4, [70, 50])] {
            let mut referral = rect(Kind::Refer, 40, 40, 60, 60);
            referral.refer_type = kind;
            let p = Polygons::new(
                vec![
                    referral,
                    rect(Kind::Path, 20, 20, 80, 30),
                    rect(Kind::Path, 20, 70, 80, 80),
                    rect(Kind::Path, 20, 20, 30, 80),
                    rect(Kind::Path, 70, 20, 80, 80),
                ],
                0,
            );
            assert_eq!(
                resolve(&p, [50, 50], [0, 0], [100, 100]).unwrap(),
                Some(target)
            );
            let mut default = p.clone();
            default.active[0].enabled = false;
            default.default_refer = kind;
            assert_eq!(
                resolve(&default, [50, 50], [0, 0], [100, 100]).unwrap(),
                Some(target)
            );
        }
        let mut referral = rect(Kind::Refer, 0, 0, 100, 100);
        referral.refer_type = 5;
        referral.node_position = [50, 50];
        let p = Polygons::new(vec![referral, rect(Kind::Path, 40, 40, 60, 60)], 0);
        assert_eq!(
            resolve(&p, [10, 10], [0, 0], [100, 100]).unwrap(),
            Some([50, 50])
        );
        assert_eq!(
            resolve(&p, [45, 45], [0, 0], [100, 100]).unwrap(),
            Some([45, 45])
        );
    }
    #[test]
    fn blocked_search_down_then_up_and_no_destination() {
        let p = Polygons::new(
            vec![
                rect(Kind::Path, 0, 0, 20, 100),
                rect(Kind::Block, 0, 40, 20, 60),
            ],
            0,
        );
        assert_eq!(
            resolve(&p, [10, 50], [0, 0], [320, 200]).unwrap(),
            Some([10, 61])
        );
        let p = Polygons::new(
            vec![
                rect(Kind::Path, 0, 0, 20, 60),
                rect(Kind::Block, 0, 40, 20, 60),
            ],
            0,
        );
        assert_eq!(
            resolve(&p, [10, 50], [0, 0], [320, 200]).unwrap(),
            Some([10, 39])
        );
        let p = Polygons::new(
            vec![
                rect(Kind::Path, 0, 40, 20, 60),
                rect(Kind::Block, 0, 40, 20, 60),
            ],
            0,
        );
        assert_eq!(resolve(&p, [10, 50], [0, 0], [320, 200]).unwrap(), None);
        let p = Polygons::new(
            vec![
                rect(Kind::Path, 0, 0, 20, 400),
                rect(Kind::Block, 0, 0, 20, 400),
            ],
            0,
        );
        assert!(resolve(&p, [10, 150], [0, 100], [320, 400]).is_err());
    }
    #[test]
    fn search_is_bounded_for_untrusted_coordinates() {
        let p = Polygons::new(vec![], 2);
        assert!(resolve(&p, [0, i32::MIN], [0, 0], [320, i32::MAX]).is_err());
    }
}
