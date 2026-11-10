//! Turns a vertex-animated `LodMesh` plus a set of skins into render-ready data: one texture
//! atlas, one index buffer and per-frame positions / normals in game space.
use super::mesh::*;
use super::texture::TexImage;

pub const CELL: u32 = 256;

pub struct Character {
    pub name: String,
    pub frames: usize,
    /// source vertices per frame
    pub frame_verts: usize,
    /// per frame, per source vertex (metres, Y up, facing -Z)
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    /// output vertex -> source vertex
    pub src: Vec<u32>,
    pub uv: Vec<[f32; 2]>,
    /// counter-clockwise triangles
    pub indices: Vec<u32>,
    pub atlas_size: u32,
    pub atlas: Vec<u8>,
    pub seqs: Vec<AnimSeq>,
    /// source vertex indices of the weapon attachment triangle
    pub weapon_tri: Option<[u32; 3]>,
    /// height of the feet below the mesh origin in the first frame
    pub foot: f32,
}

/// Bilinear resample of an RGBA image into a `CELL` x `CELL` square.
fn resample(t: &TexImage) -> Vec<u8> {
    let mut out = vec![0u8; (CELL * CELL * 4) as usize];
    let (w, h) = (t.width as f32, t.height as f32);
    for y in 0..CELL {
        for x in 0..CELL {
            let fx = ((x as f32 + 0.5) / CELL as f32 * w - 0.5).max(0.0);
            let fy = ((y as f32 + 0.5) / CELL as f32 * h - 0.5).max(0.0);
            let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
            let (x1, y1) = ((x0 + 1).min(t.width - 1), (y0 + 1).min(t.height - 1));
            let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
            let px = |xx: u32, yy: u32, c: usize| t.rgba[((yy * t.width + xx) * 4) as usize + c] as f32;
            let o = ((y * CELL + x) * 4) as usize;
            for c in 0..4 {
                let top = px(x0, y0, c) * (1.0 - ax) + px(x1, y0, c) * ax;
                let bot = px(x0, y1, c) * (1.0 - ax) + px(x1, y1, c) * ax;
                out[o + c] = (top * (1.0 - ay) + bot * ay + 0.5) as u8;
            }
            // keep the cut-out crisp: masked texels are either in or out
            out[o + 3] = if out[o + 3] >= 128 { 255 } else { 0 };
        }
    }
    out
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l < 1e-9 {
        [0.0, 1.0, 0.0]
    } else {
        [a[0] / l, a[1] / l, a[2] / l]
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `skins[k]` is the texture of material texture slot `k` (`None` = not drawn: lenses,
/// glasses frames and other modulated / translucent bits, or slots a character leaves empty).
pub fn build_character(mesh: &LodMesh, skins: &[Option<&TexImage>], units_per_m: f32) -> Character {
    // ---- atlas: one cell per distinct texture
    let mut cells: Vec<&TexImage> = vec![];
    let mut slot_cell: Vec<Option<usize>> = vec![None; skins.len()];
    for (k, s) in skins.iter().enumerate() {
        if let Some(t) = s {
            let idx = match cells.iter().position(|c| c.name == t.name) {
                Some(i) => i,
                None => {
                    cells.push(t);
                    cells.len() - 1
                }
            };
            slot_cell[k] = Some(idx);
        }
    }
    let grid = match cells.len() {
        0 | 1 => 1u32,
        2..=4 => 2,
        _ => 4,
    };
    let atlas_size = grid * CELL;
    let mut atlas = vec![0u8; (atlas_size * atlas_size * 4) as usize];
    for (i, t) in cells.iter().enumerate() {
        let img = resample(t);
        let (cx, cy) = (i as u32 % grid, i as u32 / grid);
        for y in 0..CELL {
            let d = (((cy * CELL + y) * atlas_size + cx * CELL) * 4) as usize;
            let s = (y * CELL * 4) as usize;
            atlas[d..d + (CELL * 4) as usize].copy_from_slice(&img[s..s + (CELL * 4) as usize]);
        }
    }

    // ---- geometry: one output vertex per (wedge, atlas cell)
    let mut src = vec![];
    let mut uv = vec![];
    let mut indices = vec![];
    let mut remap: std::collections::HashMap<(u16, usize), u32> = Default::default();
    for f in &mesh.faces {
        let Some(mat) = mesh.materials.get(f.material as usize) else { continue };
        if mat.flags & PF_INVISIBLE != 0 {
            continue;
        }
        let Some(Some(cell)) = slot_cell.get(mat.texture.max(0) as usize).copied() else { continue };
        let (cx, cy) = ((cell as u32 % grid) as f32, (cell as u32 / grid) as f32);
        let mut tri = [0u32; 3];
        for (k, &w) in f.wedges.iter().enumerate() {
            let id = *remap.entry((w, cell)).or_insert_with(|| {
                let wd = mesh.wedges[w as usize];
                src.push(wd.vertex as u32);
                let u = (cx * CELL as f32 + 0.5 + wd.u as f32 / 255.0 * (CELL as f32 - 1.0)) / atlas_size as f32;
                let v = (cy * CELL as f32 + 0.5 + wd.v as f32 / 255.0 * (CELL as f32 - 1.0)) / atlas_size as f32;
                uv.push([u, v]);
                (src.len() - 1) as u32
            });
            tri[k] = id;
        }
        // Unreal winding is clockwise once converted to a right-handed space
        indices.extend_from_slice(&[tri[0], tri[2], tri[1]]);
    }

    // ---- frames
    let fv = mesh.frame_verts;
    let mut pos = Vec::with_capacity(mesh.frames * fv);
    let mut nrm = Vec::with_capacity(mesh.frames * fv);
    for fi in 0..mesh.frames {
        let base = pos.len();
        for v in mesh.frame(fi) {
            pos.push(mesh.to_world(*v, units_per_m));
        }
        let mut acc = vec![[0.0f32; 3]; fv];
        for t in indices.chunks(3) {
            let (a, b, c) = (src[t[0] as usize] as usize, src[t[1] as usize] as usize, src[t[2] as usize] as usize);
            let n = cross(sub(pos[base + b], pos[base + a]), sub(pos[base + c], pos[base + a]));
            for k in [a, b, c] {
                for j in 0..3 {
                    acc[k][j] += n[j];
                }
            }
        }
        nrm.extend(acc.into_iter().map(norm));
    }
    let foot = -(0..fv).map(|i| pos[i][1]).fold(f32::MAX, f32::min);
    let weapon_tri = mesh.special_faces.first().map(|f| [f[0] as u32, f[1] as u32, f[2] as u32]).filter(|f| f.iter().all(|&i| (i as usize) < fv));
    Character {
        name: mesh.name.clone(),
        frames: mesh.frames,
        frame_verts: fv,
        pos,
        nrm,
        src,
        uv,
        indices,
        atlas_size,
        atlas,
        seqs: mesh.seqs.clone(),
        weapon_tri,
        foot,
    }
}

impl Character {
    pub fn seq(&self, name: &str) -> Option<&AnimSeq> {
        self.seqs.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// Source-vertex pose blended between two frames.
    pub fn pose(&self, f0: usize, f1: usize, t: f32, out: &mut Vec<[f32; 3]>, out_n: &mut Vec<[f32; 3]>) {
        let fv = self.frame_verts;
        let (a, b) = (f0.min(self.frames - 1) * fv, f1.min(self.frames - 1) * fv);
        out.clear();
        out_n.clear();
        for i in 0..fv {
            let (p, q) = (self.pos[a + i], self.pos[b + i]);
            out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, p[2] + (q[2] - p[2]) * t]);
            let (m, n) = (self.nrm[a + i], self.nrm[b + i]);
            out_n.push(norm([m[0] + (n[0] - m[0]) * t, m[1] + (n[1] - m[1]) * t, m[2] + (n[2] - m[2]) * t]));
        }
    }

    /// Weapon grip frame from a source-vertex pose: (origin, forward, down), both unit and
    /// orthogonal. The triangle's first vertex sits at the top of the grip, the second
    /// toward the muzzle and the third down the grip.
    pub fn weapon_frame(&self, pose: &[[f32; 3]]) -> Option<([f32; 3], [f32; 3], [f32; 3])> {
        let [i0, i1, i2] = self.weapon_tri?;
        let (p0, p1, p2) = (pose[i0 as usize], pose[i1 as usize], pose[i2 as usize]);
        let down = norm(sub(p2, p0));
        let a = sub(p1, p0);
        let d = dot(a, down);
        let fwd = norm([a[0] - down[0] * d, a[1] - down[1] * d, a[2] - down[2] * d]);
        Some((p0, fwd, down))
    }
}

/// Where a sequence is at `time` seconds: (frame, next frame, blend). One-shot sequences hold
/// their last frame.
pub fn seq_frames(s: &AnimSeq, time: f32, looping: bool) -> (usize, usize, f32) {
    let n = s.frames.max(1) as f32;
    let rate = if s.rate > 0.0 { s.rate } else { 10.0 };
    let mut f = time.max(0.0) * rate;
    if looping {
        f %= n;
    } else if f >= n - 1.0 {
        let last = s.start as usize + s.frames.max(1) as usize - 1;
        return (last, last, 0.0);
    }
    let i = f.floor();
    let t = f - i;
    let a = s.start as usize + i as usize;
    let b = if looping { s.start as usize + ((i as usize + 1) % s.frames.max(1) as usize) } else { a + 1 };
    (a, b, t)
}
