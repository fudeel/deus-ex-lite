//! PBR maps for an original (unlit, palettised) Deus Ex texture: the albedo is used as is,
//! a normal map is derived from its luminance and a roughness / metal map from per-surface
//! settings, so old textures still pick up the game's dynamic lights and reflections.
use super::texture::TexImage;

pub struct PbrMaps {
    pub width: u32,
    pub height: u32,
    /// RGBA8 mip chains (level 0 first)
    pub albedo: Vec<Vec<u8>>,
    pub normal: Vec<Vec<u8>>,
    pub mr: Vec<Vec<u8>>,
}

#[derive(Clone, Copy, Debug)]
pub struct SurfaceLook {
    /// normal map strength (0 = flat)
    pub bump: f32,
    pub rough: f32,
    /// how much darker texels get rougher (grime) and brighter ones smoother
    pub rough_var: f32,
    pub metal: f32,
}

fn to_u8(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// 2x2 box-filtered mip chain of a power-of-two (or any even) sized RGBA image.
pub fn mip_chain(base: Vec<u8>, w: u32, h: u32, renorm: bool) -> Vec<Vec<u8>> {
    let mut out = vec![base];
    let (mut cw, mut ch) = (w, h);
    while cw > 1 || ch > 1 {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let prev = out.last().unwrap();
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0u32; 4];
                let mut n = 0;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = ((x * 2 + dx).min(cw - 1), (y * 2 + dy).min(ch - 1));
                    let i = ((sy * cw + sx) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += prev[i + c] as u32;
                    }
                    n += 1;
                }
                let o = ((y * nw + x) * 4) as usize;
                if renorm {
                    let f = |v: u32| v as f32 / (n as f32 * 255.0) * 2.0 - 1.0;
                    let (vx, vy, vz) = (f(acc[0]), f(acc[1]), f(acc[2]));
                    let l = (vx * vx + vy * vy + vz * vz).sqrt().max(1e-6);
                    next[o] = to_u8(vx / l * 0.5 + 0.5);
                    next[o + 1] = to_u8(vy / l * 0.5 + 0.5);
                    next[o + 2] = to_u8(vz / l * 0.5 + 0.5);
                    next[o + 3] = 255;
                } else {
                    for c in 0..4 {
                        next[o + c] = ((acc[c] + n / 2) / n) as u8;
                    }
                }
            }
        }
        out.push(next);
        cw = nw;
        ch = nh;
    }
    out
}

pub fn build_pbr(t: &TexImage, look: SurfaceLook) -> PbrMaps {
    let (w, h) = (t.width, t.height);
    let n = (w * h) as usize;
    let lum: Vec<f32> = t.rgba.chunks(4).map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0).collect();
    // light blur so palette dithering does not turn into normal-map noise
    let at = |x: i32, y: i32| lum[((y.rem_euclid(h as i32) as u32 * w) + x.rem_euclid(w as i32) as u32) as usize];
    let mut hgt = vec![0.0f32; n];
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            hgt[(y as u32 * w + x as u32) as usize] = (at(x, y) * 4.0 + at(x - 1, y) + at(x + 1, y) + at(x, y - 1) + at(x, y + 1)) / 8.0;
        }
    }
    let hh = |x: i32, y: i32| hgt[((y.rem_euclid(h as i32) as u32 * w) + x.rem_euclid(w as i32) as u32) as usize];
    let mut normal = vec![0u8; n * 4];
    let mut mr = vec![0u8; n * 4];
    // Deus Ex textures are small (64-256 px) and their luminance steps are shallower than
    // the procedural height fields, so they get twice the procedural per-texel strength
    let k = look.bump * w as f32 / 128.0;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let i = (y as u32 * w + x as u32) as usize;
            let dx = (hh(x + 1, y) - hh(x - 1, y)) * 0.5 * k;
            let dy = (hh(x, y + 1) - hh(x, y - 1)) * 0.5 * k;
            let l = (dx * dx + dy * dy + 1.0).sqrt();
            normal[i * 4] = to_u8(-dx / l * 0.5 + 0.5);
            normal[i * 4 + 1] = to_u8(dy / l * 0.5 + 0.5);
            normal[i * 4 + 2] = to_u8(1.0 / l * 0.5 + 0.5);
            normal[i * 4 + 3] = 255;
            mr[i * 4] = 255;
            mr[i * 4 + 1] = to_u8(look.rough + (0.45 - lum[i]) * look.rough_var);
            mr[i * 4 + 2] = to_u8(look.metal);
            mr[i * 4 + 3] = 255;
        }
    }
    PbrMaps { width: w, height: h, albedo: mip_chain(t.rgba.clone(), w, h, false), normal: mip_chain(normal, w, h, true), mr: mip_chain(mr, w, h, false) }
}
