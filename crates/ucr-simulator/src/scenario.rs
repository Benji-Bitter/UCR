//! Scenario geometry — positions, obstacles, mobility.

use serde::{Deserialize, Serialize};

/// Ground position in metres on a local tangent plane.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pos {
    pub x: f32,
    pub y: f32,
}

impl Pos {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance(&self, o: &Pos) -> f32 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
}

/// A rectangular attenuating obstacle (hill, building block, forest).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Obstacle {
    /// Centre.
    pub pos: Pos,
    /// Half-extents.
    pub half: Pos,
    /// Extra attenuation (dB) for a path crossing the rectangle.
    pub atten_db: f32,
}

impl Obstacle {
    /// Does the segment a→b cross this rectangle? (slab test)
    pub fn crosses(&self, a: Pos, b: Pos) -> bool {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let mut t_min = 0.0f32;
        let mut t_max = 1.0f32;
        for (p, d, c, h) in [
            (a.x, dx, self.pos.x, self.half.x),
            (a.y, dy, self.pos.y, self.half.y),
        ] {
            if d.abs() < 1e-9 {
                if (p - c).abs() > h {
                    return false;
                }
            } else {
                let inv = 1.0 / d;
                let mut t1 = (c - h - p) * inv;
                let mut t2 = (c + h - p) * inv;
                if t1 > t2 {
                    std::mem::swap(&mut t1, &mut t2);
                }
                t_min = t_min.max(t1);
                t_max = t_max.min(t2);
                if t_min > t_max {
                    return false;
                }
            }
        }
        true
    }
}

/// Node mobility model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Mobility {
    /// Stationary.
    Fixed,
    /// Linear waypoint loop at `speed_mps` metres/s.
    Waypoints { points: Vec<Pos>, speed_mps: f32 },
}

impl Mobility {
    /// Advance `pos` along the waypoint loop; `dt_ms` elapsed.
    /// `progress` is per-node state (current segment index, fractional pos).
    pub fn advance(&self, pos: &mut Pos, progress: &mut (usize, f32), dt_ms: u64) {
        let Mobility::Waypoints { points, speed_mps } = self else {
            return;
        };
        if points.len() < 2 {
            return;
        }
        let mut remaining = *speed_mps * (dt_ms as f32) / 1000.0;
        loop {
            let (seg, frac) = *progress;
            let a = points[seg];
            let b = points[(seg + 1) % points.len()];
            let seg_len = a.distance(&b).max(1e-6);
            let seg_left = seg_len * (1.0 - frac);
            if remaining < seg_left {
                let new_frac = frac + remaining / seg_len;
                progress.1 = new_frac;
                pos.x = a.x + (b.x - a.x) * new_frac;
                pos.y = a.y + (b.y - a.y) * new_frac;
                break;
            }
            remaining -= seg_left;
            *progress = ((seg + 1) % points.len(), 0.0);
        }
    }
}
