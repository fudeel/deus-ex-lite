//! Collision world: static level columns (grid), static props, moving doors and shootable targets.
use bevy::prelude::*;

use crate::level::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Aabb { min, max }
    }
    pub fn centered(c: Vec3, half: Vec3) -> Self {
        Aabb { min: c - half, max: c + half }
    }
    pub fn overlaps(&self, o: &Aabb) -> bool {
        self.min.x < o.max.x && self.max.x > o.min.x && self.min.y < o.max.y && self.max.y > o.min.y && self.min.z < o.max.z && self.max.z > o.min.z
    }
    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }
    pub fn shrink(&self, e: f32) -> Aabb {
        Aabb { min: self.min + Vec3::splat(e), max: self.max - Vec3::splat(e) }
    }
    pub fn union(&self, o: &Aabb) -> Aabb {
        Aabb { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    /// Slab test. Returns (t, surface normal).
    pub fn ray(&self, o: Vec3, d: Vec3, tmax: f32) -> Option<(f32, Vec3)> {
        let mut tmin = 0.0f32;
        let mut tmx = tmax;
        let mut n = Vec3::ZERO;
        for a in 0..3 {
            let (oa, da, mn, mx) = (o[a], d[a], self.min[a], self.max[a]);
            if da.abs() < 1e-9 {
                if oa < mn || oa > mx {
                    return None;
                }
                continue;
            }
            let inv = 1.0 / da;
            let mut t0 = (mn - oa) * inv;
            let mut t1 = (mx - oa) * inv;
            let mut sign = -1.0;
            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
                sign = 1.0;
            }
            if t0 > tmin {
                tmin = t0;
                n = Vec3::ZERO;
                n[a] = sign;
            }
            tmx = tmx.min(t1);
            if tmin > tmx {
                return None;
            }
        }
        if n == Vec3::ZERO {
            n = -d;
        }
        Some((tmin, n))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Body {
    World,
    Glass,
    Door(u16),
    Guard(u16, bool),
    Light(u16),
    Camera(u16),
    Turret(u16),
    Player(u8),
    /// head hit box of a team-match player (headshots)
    PlayerHead(u8),
    Npc(u16),
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f32,
    pub pos: Vec3,
    pub normal: Vec3,
    pub body: Body,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct MoveOut {
    pub pos: Vec3,
    pub grounded: bool,
    pub hit_ceiling: bool,
    pub hit_wall: bool,
    pub stepped: bool,
}

#[derive(Resource, Default)]
pub struct Physics {
    pub w: i32,
    pub h: i32,
    /// collidable intervals per cell: (y0, y1, is_glass)
    pub cols: Vec<Vec<(f32, f32, bool)>>,
    pub props: Vec<Aabb>,
    /// solid moving things (doors), rebuilt every frame
    pub dynamic: Vec<(Aabb, Body)>,
    /// shootable but walk-through things (guards, lights, devices, players)
    pub targets: Vec<(Aabb, Body)>,
}

impl Physics {
    pub fn from_level(lvl: &Level) -> Self {
        let cols = (0..lvl.h)
            .flat_map(|z| (0..lvl.w).map(move |x| (x, z)))
            .map(|(x, z)| {
                let t = lvl.tile(x, z);
                lvl.cols[lvl.idx(x, z)]
                    .iter()
                    .filter(|i| i.collide)
                    .map(|i| (i.y0, i.y1, t == Tile::Glass && i.part == Part::Invisible))
                    .collect()
            })
            .collect();
        Physics { w: lvl.w, h: lvl.h, cols, props: vec![], dynamic: vec![], targets: vec![] }
    }

    fn cell_box(x: i32, z: i32, y0: f32, y1: f32) -> Aabb {
        Aabb::new(Vec3::new(x as f32 * CELL, y0, z as f32 * CELL), Vec3::new((x + 1) as f32 * CELL, y1, (z + 1) as f32 * CELL))
    }

    /// All solid boxes overlapping `q`.
    pub fn collect(&self, q: &Aabb, out: &mut Vec<Aabb>) {
        out.clear();
        let x0 = (q.min.x / CELL).floor() as i32;
        let x1 = (q.max.x / CELL).floor() as i32;
        let z0 = (q.min.z / CELL).floor() as i32;
        let z1 = (q.max.z / CELL).floor() as i32;
        for z in z0..=z1 {
            for x in x0..=x1 {
                if x < 0 || z < 0 || x >= self.w || z >= self.h {
                    // outside the map is solid
                    let b = Self::cell_box(x, z, -100.0, 100.0);
                    if b.overlaps(q) {
                        out.push(b);
                    }
                    continue;
                }
                for &(y0, y1, _) in &self.cols[(z * self.w + x) as usize] {
                    if y1 > q.min.y && y0 < q.max.y {
                        out.push(Self::cell_box(x, z, y0, y1));
                    }
                }
            }
        }
        for p in &self.props {
            if p.overlaps(q) {
                out.push(*p);
            }
        }
        for (d, _) in &self.dynamic {
            if d.overlaps(q) {
                out.push(*d);
            }
        }
    }

    pub fn body_box(feet: Vec3, hw: f32, h: f32) -> Aabb {
        Aabb::new(feet - Vec3::new(hw, 0.0, hw), feet + Vec3::new(hw, h, hw))
    }

    pub fn is_free(&self, feet: Vec3, hw: f32, h: f32) -> bool {
        let mut buf = Vec::new();
        self.collect(&Self::body_box(feet, hw, h).shrink(0.002), &mut buf);
        buf.is_empty()
    }

    fn sweep_axis(&self, feet: &mut Vec3, hw: f32, h: f32, axis: usize, amount: f32, buf: &mut Vec<Aabb>) -> bool {
        if amount == 0.0 {
            return false;
        }
        let old = Self::body_box(*feet, hw, h);
        let mut target = *feet;
        target[axis] += amount;
        let nb = Self::body_box(target, hw, h);
        self.collect(&old.union(&nb).shrink(0.0005), buf);
        let old_in = old.shrink(0.003);
        let (ext_pos, ext_neg) = if axis == 1 { (h, 0.0) } else { (hw, hw) };
        let mut hit = false;
        for s in buf.iter() {
            if s.overlaps(&old_in) {
                continue;
            }
            if amount > 0.0 {
                let lim = s.min[axis] - ext_pos - 0.001;
                if lim < target[axis] {
                    target[axis] = lim;
                    hit = true;
                }
            } else {
                let lim = s.max[axis] + ext_neg + 0.001;
                if lim > target[axis] {
                    target[axis] = lim;
                    hit = true;
                }
            }
        }
        // never move backwards because of a clamp
        if amount > 0.0 {
            target[axis] = target[axis].max(feet[axis]);
        } else {
            target[axis] = target[axis].min(feet[axis]);
        }
        *feet = target;
        hit
    }

    /// Kinematic box movement with step-up.
    pub fn move_body(&self, feet: Vec3, hw: f32, h: f32, delta: Vec3, step_h: f32) -> MoveOut {
        let mut buf = Vec::with_capacity(16);
        let mut out = MoveOut::default();
        let mut p = feet;
        for axis in [0usize, 2] {
            let amt = delta[axis];
            if amt == 0.0 {
                continue;
            }
            let mut trial = p;
            let hit = self.sweep_axis(&mut trial, hw, h, axis, amt, &mut buf);
            if hit && step_h > 0.0 {
                let mut up = p;
                self.sweep_axis(&mut up, hw, h, 1, step_h, &mut buf);
                let raised = up.y - p.y;
                if raised > 0.05 {
                    let mut fwd = up;
                    self.sweep_axis(&mut fwd, hw, h, axis, amt, &mut buf);
                    let mut down = fwd;
                    self.sweep_axis(&mut down, hw, h, 1, -raised, &mut buf);
                    if (down[axis] - p[axis]).abs() > (trial[axis] - p[axis]).abs() + 0.01 {
                        trial = down;
                        out.stepped = true;
                    }
                }
            }
            if hit && !out.stepped {
                out.hit_wall = true;
            }
            p = trial;
        }
        if delta.y != 0.0 {
            let hit = self.sweep_axis(&mut p, hw, h, 1, delta.y, &mut buf);
            if hit {
                if delta.y < 0.0 {
                    out.grounded = true;
                } else {
                    out.hit_ceiling = true;
                }
            }
        }
        out.pos = p;
        out
    }

    /// Grid DDA against static columns.
    fn ray_static(&self, o: Vec3, d: Vec3, max: f32, see_glass: bool) -> Option<(f32, Vec3, bool)> {
        let mut cx = (o.x / CELL).floor() as i32;
        let mut cz = (o.z / CELL).floor() as i32;
        let step_x = if d.x > 0.0 { 1 } else { -1 };
        let step_z = if d.z > 0.0 { 1 } else { -1 };
        let inf = f32::INFINITY;
        let t_dx = if d.x.abs() > 1e-9 { CELL / d.x.abs() } else { inf };
        let t_dz = if d.z.abs() > 1e-9 { CELL / d.z.abs() } else { inf };
        let mut t_mx = if d.x.abs() > 1e-9 {
            let nb = if d.x > 0.0 { (cx + 1) as f32 * CELL } else { cx as f32 * CELL };
            (nb - o.x) / d.x
        } else {
            inf
        };
        let mut t_mz = if d.z.abs() > 1e-9 {
            let nb = if d.z > 0.0 { (cz + 1) as f32 * CELL } else { cz as f32 * CELL };
            (nb - o.z) / d.z
        } else {
            inf
        };
        let mut steps = 0;
        loop {
            if cx < 0 || cz < 0 || cx >= self.w || cz >= self.h {
                return None;
            }
            let mut best: Option<(f32, Vec3, bool)> = None;
            for &(y0, y1, glass) in &self.cols[(cz * self.w + cx) as usize] {
                if glass && see_glass {
                    continue;
                }
                if let Some((t, n)) = Self::cell_box(cx, cz, y0, y1).ray(o, d, max) {
                    if best.map(|b| t < b.0).unwrap_or(true) {
                        best = Some((t, n, glass));
                    }
                }
            }
            if best.is_some() {
                return best;
            }
            let t = if t_mx < t_mz {
                let t = t_mx;
                t_mx += t_dx;
                cx += step_x;
                t
            } else {
                let t = t_mz;
                t_mz += t_dz;
                cz += step_z;
                t
            };
            steps += 1;
            if t > max || steps > 512 {
                return None;
            }
        }
    }

    /// Full raycast. `targets`: include shootable bodies. `see_glass`: glass doesn't block.
    pub fn raycast(&self, o: Vec3, d: Vec3, max: f32, targets: bool, see_glass: bool, ignore: Option<Body>) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let consider = |t: f32, n: Vec3, body: Body, best: &mut Option<Hit>| {
            if t <= max && best.map(|b| t < b.t).unwrap_or(true) {
                *best = Some(Hit { t, pos: o + d * t, normal: n, body });
            }
        };
        if let Some((t, n, glass)) = self.ray_static(o, d, max, see_glass) {
            consider(t, n, if glass { Body::Glass } else { Body::World }, &mut best);
        }
        for p in &self.props {
            if let Some((t, n)) = p.ray(o, d, max) {
                consider(t, n, Body::World, &mut best);
            }
        }
        for (b, body) in &self.dynamic {
            if Some(*body) == ignore {
                continue;
            }
            if let Some((t, n)) = b.ray(o, d, max) {
                consider(t, n, *body, &mut best);
            }
        }
        if targets {
            for (b, body) in &self.targets {
                if Some(*body) == ignore {
                    continue;
                }
                if let Some((t, n)) = b.ray(o, d, max) {
                    consider(t, n, *body, &mut best);
                }
            }
        }
        best
    }

    /// Line of sight (glass is transparent, bodies ignored).
    pub fn los(&self, a: Vec3, b: Vec3) -> bool {
        let d = b - a;
        let len = d.length();
        if len < 1e-4 {
            return true;
        }
        self.raycast(a, d / len, len - 0.05, false, true, None).is_none()
    }
}
