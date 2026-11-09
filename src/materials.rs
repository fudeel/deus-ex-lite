//! Procedural PBR textures + the shared material / mesh library.
//!
//! Every level surface gets a generated albedo, normal and metal/roughness map (with mipmaps).
//! To replace one with your own art, drop `assets/textures/<name>.png` (and optionally
//! `<name>_normal.png`) next to the game - names are listed in `surf_name`.
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::math::primitives::{Capsule3d, Cuboid, Cylinder, Sphere};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;
use std::f32::consts::TAU;

use crate::level::Surf;
use crate::util::*;

pub fn emis(r: f32, g: f32, b: f32, nits: f32) -> LinearRgba {
    LinearRgba::rgb(r * nits, g * nits, b * nits)
}

#[derive(Clone, Copy)]
pub struct Px {
    pub col: Vec3,
    pub h: f32,
    pub rough: f32,
    pub metal: f32,
    pub emit: Vec3,
    pub alpha: f32,
}

impl Px {
    fn new(col: Vec3, h: f32, rough: f32, metal: f32) -> Self {
        Px { col, h, rough, metal, emit: Vec3::ZERO, alpha: 1.0 }
    }
}

struct TexOut {
    size: u32,
    albedo: Vec<Vec<u8>>,
    normal: Vec<Vec<u8>>,
    mr: Vec<Vec<u8>>,
    emissive: Option<Vec<Vec<u8>>>,
}

fn to_u8(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn mips(base: Vec<u8>, size: u32, renorm: bool) -> Vec<Vec<u8>> {
    let mut out = vec![base];
    let mut s = size;
    while s > 1 {
        let prev = out.last().unwrap();
        let ns = s / 2;
        let mut next = vec![0u8; (ns * ns * 4) as usize];
        for y in 0..ns {
            for x in 0..ns {
                let mut acc = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let i = (((y * 2 + dy) * s + (x * 2 + dx)) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += prev[i + c] as u32;
                    }
                }
                let o = ((y * ns + x) * 4) as usize;
                if renorm {
                    let v = Vec3::new(acc[0] as f32 / 1020.0 * 2.0 - 1.0, acc[1] as f32 / 1020.0 * 2.0 - 1.0, acc[2] as f32 / 1020.0 * 2.0 - 1.0)
                        .try_normalize()
                        .unwrap_or(Vec3::Z);
                    next[o] = to_u8(v.x * 0.5 + 0.5);
                    next[o + 1] = to_u8(v.y * 0.5 + 0.5);
                    next[o + 2] = to_u8(v.z * 0.5 + 0.5);
                    next[o + 3] = 255;
                } else {
                    for c in 0..4 {
                        next[o + c] = ((acc[c] + 2) / 4) as u8;
                    }
                }
            }
        }
        out.push(next);
        s = ns;
    }
    out
}

fn generate(size: u32, nstrength: f32, f: &(dyn Fn(f32, f32) -> Px + Sync)) -> TexOut {
    let n = (size * size) as usize;
    let mut px = Vec::with_capacity(n);
    for y in 0..size {
        for x in 0..size {
            let u = (x as f32 + 0.5) / size as f32;
            let v = (y as f32 + 0.5) / size as f32;
            px.push(f(u, v));
        }
    }
    let mut albedo = vec![0u8; n * 4];
    let mut normal = vec![0u8; n * 4];
    let mut mr = vec![0u8; n * 4];
    let has_emit = px.iter().any(|p| p.emit != Vec3::ZERO);
    let mut emit = if has_emit { vec![0u8; n * 4] } else { vec![] };
    let s = size as i32;
    let hgt = |x: i32, y: i32| px[(y.rem_euclid(s) * s + x.rem_euclid(s)) as usize].h;
    for y in 0..s {
        for x in 0..s {
            let i = (y * s + x) as usize;
            let p = px[i];
            albedo[i * 4] = to_u8(p.col.x);
            albedo[i * 4 + 1] = to_u8(p.col.y);
            albedo[i * 4 + 2] = to_u8(p.col.z);
            albedo[i * 4 + 3] = to_u8(p.alpha);
            let dx = (hgt(x + 1, y) - hgt(x - 1, y)) * 0.5 * nstrength * size as f32 / 256.0;
            let dy = (hgt(x, y + 1) - hgt(x, y - 1)) * 0.5 * nstrength * size as f32 / 256.0;
            let nv = Vec3::new(-dx, dy, 1.0).normalize();
            normal[i * 4] = to_u8(nv.x * 0.5 + 0.5);
            normal[i * 4 + 1] = to_u8(nv.y * 0.5 + 0.5);
            normal[i * 4 + 2] = to_u8(nv.z * 0.5 + 0.5);
            normal[i * 4 + 3] = 255;
            mr[i * 4] = 255;
            mr[i * 4 + 1] = to_u8(p.rough);
            mr[i * 4 + 2] = to_u8(p.metal);
            mr[i * 4 + 3] = 255;
            if has_emit {
                emit[i * 4] = to_u8(p.emit.x);
                emit[i * 4 + 1] = to_u8(p.emit.y);
                emit[i * 4 + 2] = to_u8(p.emit.z);
                emit[i * 4 + 3] = 255;
            }
        }
    }
    TexOut {
        size,
        albedo: mips(albedo, size, false),
        normal: mips(normal, size, true),
        mr: mips(mr, size, false),
        emissive: if has_emit { Some(mips(emit, size, false)) } else { None },
    }
}

pub fn repeat_sampler() -> ImageSamplerDescriptor {
    ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    }
}

fn make_image(size: u32, levels: Vec<Vec<u8>>, srgb: bool) -> Image {
    let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
    let mut img = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        levels[0].clone(),
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = levels.len() as u32;
    img.data = Some(levels.concat());
    img.sampler = ImageSampler::Descriptor(repeat_sampler());
    img
}

// ------------------------------------------------------------------ texture recipes

fn aniso(u: f32, v: f32, pu: i32, pv: i32, oct: u32, seed: u32) -> f32 {
    // anisotropic tileable fbm: different lattice periods on u and v
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut norm = 0.0;
    let (mut a, mut b) = (pu, pv);
    for o in 0..oct {
        let x = u * a as f32;
        let y = v * b as f32;
        let xi = x.floor() as i32;
        let yi = y.floor() as i32;
        let xf = x - xi as f32;
        let yf = y - yi as f32;
        let su = xf * xf * (3.0 - 2.0 * xf);
        let sv = yf * yf * (3.0 - 2.0 * yf);
        let s = seed.wrapping_add(o * 17);
        let h = |i: i32, j: i32| hash2(i.rem_euclid(a), j.rem_euclid(b), s);
        let val = lerp(lerp(h(xi, yi), h(xi + 1, yi), su), lerp(h(xi, yi + 1), h(xi + 1, yi + 1), su), sv);
        sum += val * amp;
        norm += amp;
        amp *= 0.5;
        a *= 2;
        b *= 2;
    }
    sum / norm
}

fn edge_dist(fu: f32, fv: f32) -> f32 {
    fu.min(1.0 - fu).min(fv.min(1.0 - fv))
}

fn tex_concrete(u: f32, v: f32) -> Px {
    let n = fbm(u, v, 4, 5, 11);
    let fine = fbm(u, v, 32, 3, 12);
    let stain = fbm(u, v, 2, 4, 13);
    let pit = smoothstep(0.86, 0.95, value_noise(u * 160.0, v * 160.0, 160, 14));
    let joint = 1.0 - smoothstep(0.0, 0.006, edge_dist(u, v));
    let mut c = 0.40 + (n - 0.5) * 0.2 + (fine - 0.5) * 0.08 - smoothstep(0.55, 0.85, stain) * 0.14 - pit * 0.12 - joint * 0.2;
    c = c.max(0.05);
    let wet = smoothstep(0.6, 0.7, stain);
    Px::new(Vec3::new(c, c * 0.99, c * 0.96), 0.5 + (fine - 0.5) * 0.4 - pit * 0.4 - joint * 0.7, lerp(0.85, 0.35, wet) - fine * 0.05, 0.0)
}

fn tex_asphalt(u: f32, v: f32) -> Px {
    let agg = value_noise(u * 300.0, v * 300.0, 300, 21);
    let agg2 = value_noise(u * 113.0, v * 113.0, 113, 22);
    let n = fbm(u, v, 4, 5, 23);
    let puddle = smoothstep(0.57, 0.63, fbm(u, v, 3, 5, 24));
    let r = 1.0 - (fbm(u, v, 5, 4, 25) * 2.0 - 1.0).abs();
    let crack = smoothstep(0.965, 0.995, r);
    let mut c = 0.12 + (agg - 0.5) * 0.07 + (agg2 - 0.5) * 0.04 + (n - 0.5) * 0.05 - crack * 0.06;
    c = lerp(c, c * 0.55, puddle).max(0.02);
    let rough = lerp(0.62 + (agg - 0.5) * 0.25, 0.03, puddle);
    let h = lerp(0.5 + (agg - 0.5) * 0.7 + (agg2 - 0.5) * 0.3 - crack * 0.6, 0.42, puddle);
    // faded lane paint stripe along one edge
    let paint = (1.0 - smoothstep(0.0, 0.01, (u - 0.04).abs() - 0.012)) * smoothstep(0.35, 0.6, fbm(u, v, 8, 3, 26));
    let col = Vec3::new(c, c, c * 1.04).lerp(Vec3::new(0.55, 0.48, 0.2), paint * 0.6 * (1.0 - puddle));
    Px::new(col, h, rough, 0.0)
}

fn tex_floor_tile(u: f32, v: f32) -> Px {
    let (tu, tv) = (u * 4.0, v * 4.0);
    let (fu, fv) = (tu.fract(), tv.fract());
    let var = hash2(tu.floor() as i32, tv.floor() as i32, 31);
    let grout = 1.0 - smoothstep(0.0, 0.022, edge_dist(fu, fv));
    let dirt = fbm(u, v, 4, 4, 32);
    let speck = value_noise(u * 200.0, v * 200.0, 200, 33);
    let c = 0.60 + (var - 0.5) * 0.06 - smoothstep(0.5, 0.85, dirt) * 0.1 + (speck - 0.5) * 0.03;
    let base = Vec3::new(c * 0.95, c * 0.98, c);
    let col = base.lerp(Vec3::splat(0.22), grout);
    Px::new(col, 1.0 - grout * 0.8, lerp(0.22 + dirt * 0.25, 0.8, grout), 0.0)
}

fn tex_carpet(u: f32, v: f32) -> Px {
    let fib = value_noise(u * 330.0, v * 330.0, 330, 41) * 0.6 + value_noise(u * 150.0, v * 150.0, 150, 42) * 0.4;
    let check = (((u * 8.0).floor() + (v * 8.0).floor()) as i32 & 1) as f32;
    let wear = fbm(u, v, 3, 4, 43);
    let k = 0.82 + fib * 0.35 + check * 0.04 + (wear - 0.5) * 0.1;
    Px::new(Vec3::new(0.085, 0.095, 0.13) * k, fib, 0.96, 0.0)
}

fn tex_metal_floor(u: f32, v: f32) -> Px {
    let (gu, gv) = (u * 8.0, v * 8.0);
    let alt = ((gu.floor() + gv.floor()) as i32 & 1) == 1;
    let (fu, fv) = (gu.fract() - 0.5, gv.fract() - 0.5);
    let (mut a, mut b) = ((fu + fv) * 0.707, (fu - fv) * 0.707);
    if alt {
        std::mem::swap(&mut a, &mut b);
    }
    let d = (a / 0.30).powi(2) + (b / 0.085).powi(2);
    let bump = 1.0 - smoothstep(0.55, 1.0, d);
    let rust = smoothstep(0.6, 0.8, fbm(u, v, 3, 5, 51));
    let scratch = smoothstep(0.92, 1.0, aniso(u, v, 2, 90, 2, 52));
    let base = Vec3::new(0.46, 0.47, 0.49) * (0.88 + bump * 0.18 + scratch * 0.15);
    let col = base.lerp(Vec3::new(0.30, 0.17, 0.09), rust * 0.7);
    Px::new(col, bump * 0.8 + fbm(u, v, 16, 2, 53) * 0.1, lerp(0.38 - bump * 0.08, 0.85, rust), lerp(0.9, 0.2, rust))
}

fn tex_wall_lab(u: f32, v: f32) -> Px {
    let (pu, pv) = (u * 2.0, v * 2.0);
    let (fu, fv) = (pu.fract(), pv.fract());
    let var = hash2(pu.floor() as i32, pv.floor() as i32, 61);
    let seam = 1.0 - smoothstep(0.0, 0.01, edge_dist(fu, fv));
    let dirt = fbm(u, v, 3, 5, 62);
    let mut screw = 0.0f32;
    for (sx, sy) in [(0.04, 0.04), (0.96, 0.04), (0.04, 0.96), (0.96, 0.96)] {
        let d = ((fu - sx).powi(2) + (fv - sy).powi(2)).sqrt();
        screw = screw.max(1.0 - smoothstep(0.006, 0.01, d));
    }
    let c = 0.66 * (0.97 + var * 0.06) - smoothstep(0.55, 0.9, dirt) * 0.1;
    let col = Vec3::new(c * 0.97, c, c * 1.02).lerp(Vec3::splat(0.2), seam.max(screw * 0.7));
    Px::new(col, 1.0 - seam * 0.9 - screw * 0.3, 0.42 + dirt * 0.1, screw * 0.8)
}

fn tex_wall_office(u: f32, v: f32) -> Px {
    let pu = u * 8.0;
    let plank = pu.floor();
    let fu = pu.fract();
    let grain = aniso(fu * 0.999 + plank * 0.131, v, 3, 24, 4, 71);
    let rings = (grain * 9.0 + plank * 0.37).fract();
    let gap = 1.0 - smoothstep(0.0, 0.03, fu.min(1.0 - fu));
    let tone = 0.75 + hash2(plank as i32, 0, 72) * 0.35;
    let base = Vec3::new(0.24, 0.13, 0.065) * tone * (0.82 + rings * 0.3);
    let col = base.lerp(Vec3::splat(0.03), gap);
    Px::new(col, 1.0 - gap * 0.8 + rings * 0.05, 0.5 + gap * 0.3, 0.0)
}

fn tex_wall_metal(u: f32, v: f32) -> Px {
    let rib = (u * 16.0 * TAU).sin() * 0.5 + 0.5;
    let streak = aniso(u, v, 48, 3, 3, 81);
    let rust = smoothstep(0.58, 0.82, fbm(u, v, 4, 5, 82) * 0.7 + streak * 0.4);
    let base = Vec3::new(0.33, 0.37, 0.36) * (0.82 + rib * 0.22);
    let col = base.lerp(Vec3::new(0.30, 0.15, 0.07), rust);
    Px::new(col, rib * 0.9 + rust * 0.1, 0.45 + rust * 0.4, 0.65 * (1.0 - rust))
}

fn tex_brick(u: f32, v: f32) -> Px {
    let rows = 28.0;
    let cols = 8.0;
    let rv = v * rows;
    let row = rv.floor();
    let fv = rv.fract();
    let off = if (row as i32) & 1 == 1 { 0.5 } else { 0.0 };
    let ru = u * cols + off;
    let colid = (ru.floor() as i32).rem_euclid(cols as i32);
    let fu = ru.fract();
    let mortar = (1.0 - smoothstep(0.0, 0.04, fu.min(1.0 - fu))).max(1.0 - smoothstep(0.0, 0.13, fv.min(1.0 - fv)));
    let var = hash2(colid, row as i32, 91);
    let n = fbm(u, v, 16, 3, 92);
    let streak = smoothstep(0.55, 0.8, aniso(u, v, 20, 2, 3, 93));
    let burnt = if var > 0.85 { 0.55 } else { 1.0 };
    let brick = Vec3::new(0.32, 0.13, 0.085) * (0.7 + var * 0.45) * burnt * (0.9 + n * 0.2);
    let col = brick.lerp(Vec3::splat(0.25), mortar) * (1.0 - streak * 0.35);
    Px::new(col, (1.0 - mortar) * 0.75 + n * 0.25, lerp(0.75, 0.92, mortar) - streak * 0.35, 0.0)
}

fn tex_boundary(u: f32, v: f32) -> Px {
    let (pu, pv) = (u * 2.0, v * 2.0);
    let (fu, fv) = (pu.fract(), pv.fract());
    let seam = 1.0 - smoothstep(0.0, 0.006, edge_dist(fu, fv));
    let mut hole = 0.0f32;
    for i in 0..3 {
        for j in 0..3 {
            let (hx, hy) = ((i as f32 + 0.5) / 3.0, (j as f32 + 0.5) / 3.0);
            let d = ((fu - hx).powi(2) + (fv - hy).powi(2)).sqrt();
            hole = hole.max(1.0 - smoothstep(0.012, 0.018, d));
        }
    }
    let streak = aniso(u, v, 40, 2, 4, 101);
    let n = fbm(u, v, 8, 4, 102);
    let c = 0.30 + (n - 0.5) * 0.12 - smoothstep(0.45, 0.85, streak) * 0.14 - seam * 0.15 - hole * 0.2;
    Px::new(Vec3::new(c, c, c * 1.02), 0.5 + n * 0.3 - seam * 0.6 - hole * 0.8, 0.85 - smoothstep(0.6, 0.9, streak) * 0.4, 0.0)
}

fn tex_ceiling(u: f32, v: f32) -> Px {
    let (tu, tv) = (u * 4.0, v * 4.0);
    let (fu, fv) = (tu.fract(), tv.fract());
    let bar = 1.0 - smoothstep(0.0, 0.025, edge_dist(fu, fv));
    let speck = smoothstep(0.75, 0.8, value_noise(u * 260.0, v * 260.0, 260, 111));
    let stain = smoothstep(0.62, 0.85, fbm(u, v, 3, 4, 112));
    let tile = Vec3::new(0.56, 0.55, 0.52) * (1.0 - speck * 0.35 - stain * 0.25);
    let col = tile.lerp(Vec3::new(0.62, 0.63, 0.64), bar);
    Px::new(col, 0.6 - bar * 0.5 - speck * 0.3, lerp(0.95, 0.4, bar), bar * 0.8)
}

fn tex_ceiling_metal(u: f32, v: f32) -> Px {
    let rib = (v * 10.0 * TAU).sin() * 0.5 + 0.5;
    let beam = 1.0 - smoothstep(0.0, 0.05, u.min(1.0 - u));
    let n = fbm(u, v, 4, 4, 121);
    let col = Vec3::new(0.16, 0.17, 0.18) * (0.8 + rib * 0.3 + n * 0.2) * (1.0 - beam * 0.4);
    Px::new(col, rib * 0.8 + beam, 0.6, 0.5)
}

fn tex_roof(u: f32, v: f32) -> Px {
    let g = value_noise(u * 200.0, v * 200.0, 200, 131);
    let n = fbm(u, v, 4, 4, 132);
    let c = 0.12 + g * 0.06 + n * 0.04;
    Px::new(Vec3::splat(c), g, 0.9, 0.0)
}

fn tex_vent(u: f32, v: f32) -> Px {
    let spangle = (value_noise(u * 18.0, v * 18.0, 18, 141) * 5.0).floor() / 5.0;
    let seam = 1.0 - smoothstep(0.0, 0.012, u.min(1.0 - u));
    let mut rivet = 0.0f32;
    for k in 0..8 {
        let ry = (k as f32 + 0.5) / 8.0;
        for rx in [0.035, 0.965] {
            let d = ((u - rx).powi(2) + (v - ry).powi(2)).sqrt();
            rivet = rivet.max(1.0 - smoothstep(0.008, 0.013, d));
        }
    }
    let dust = smoothstep(0.5, 0.9, fbm(u, v, 4, 4, 142));
    let col = Vec3::splat(0.5 + spangle * 0.08) * (1.0 - dust * 0.4) * (1.0 - seam * 0.5);
    Px::new(col, 0.5 + rivet * 0.5 - seam * 0.5, 0.35 + dust * 0.4, 0.85 - dust * 0.5)
}

fn tex_crate(u: f32, v: f32) -> Px {
    // texture covers a 2m x 1m crate face
    let bu = 0.035;
    let bv = 0.08;
    let frame = (u < bu || u > 1.0 - bu || v < bv || v > 1.0 - bv) as i32 as f32;
    let plank_v = (v * 4.0).fract();
    let plank_gap = 1.0 - smoothstep(0.0, 0.03, plank_v.min(1.0 - plank_v));
    let brace = (1.0 - smoothstep(0.0, 0.035, ((u - 0.5) * 0.5 - (v - 0.5)).abs() * 0.7)) * (1.0 - frame);
    let grain = aniso(u, v, 40, 4, 4, 151);
    let rings = (grain * 7.0).fract();
    let tone = if frame > 0.5 || brace > 0.5 { 0.7 } else { 1.0 };
    let base = Vec3::new(0.42, 0.29, 0.16) * tone * (0.8 + rings * 0.3);
    let col = base * (1.0 - plank_gap * (1.0 - frame) * 0.6);
    let stencil = (u > 0.38 && u < 0.62 && v > 0.36 && v < 0.64 && brace < 0.5 && frame < 0.5) as i32 as f32
        * smoothstep(0.4, 0.6, value_noise(u * 60.0, v * 30.0, 60, 152));
    let col = col.lerp(Vec3::new(0.06, 0.05, 0.04), stencil * 0.5);
    Px::new(col, 0.5 + frame * 0.3 + brace * 0.3 - plank_gap * 0.4 + rings * 0.05, 0.75, 0.0)
}

fn tex_container(u: f32, v: f32) -> Px {
    let r = (u * 10.0).fract();
    let rib = smoothstep(0.1, 0.25, r) * (1.0 - smoothstep(0.6, 0.75, r));
    let rust = smoothstep(0.6, 0.85, fbm(u, v, 4, 5, 161) + aniso(u, v, 30, 3, 2, 162) * 0.25);
    let scratch = smoothstep(0.9, 1.0, aniso(u, v, 3, 60, 2, 163));
    let paint = Vec3::splat(0.78) * (0.9 + rib * 0.12) * (1.0 - scratch * 0.3);
    let col = paint.lerp(Vec3::new(0.35, 0.2, 0.12), rust);
    Px::new(col, rib * 0.9, lerp(0.5, 0.85, rust), lerp(0.25, 0.1, rust))
}

fn tex_counter(u: f32, v: f32) -> Px {
    let n = value_noise(u * 120.0, v * 120.0, 120, 171);
    let edge = 1.0 - smoothstep(0.0, 0.04, v.min(1.0 - v));
    let col = Vec3::new(0.11, 0.115, 0.125) * (0.9 + n * 0.2) + Vec3::splat(edge * 0.25);
    Px::new(col, 0.5 + edge * 0.4, 0.3 - edge * 0.1, edge * 0.9)
}

fn tex_rack(u: f32, v: f32) -> Px {
    let bay = (u * 2.0).fract();
    let frame = (bay < 0.06 || bay > 0.94 || v < 0.03 || v > 0.97) as i32 as f32;
    let unit = (v * 24.0).floor();
    let fv = (v * 24.0).fract();
    let unit_line = 1.0 - smoothstep(0.0, 0.08, fv.min(1.0 - fv));
    let bay_id = (u * 2.0).floor() as i32;
    let mut emit = Vec3::ZERO;
    if frame < 0.5 {
        for k in 0..4 {
            let lx = 0.12 + k as f32 * 0.05;
            let on = hash2(bay_id * 50 + k, unit as i32, 181);
            if on > 0.45 && (bay - lx).abs() < 0.012 && (fv - 0.5).abs() < 0.18 {
                emit = match (on * 10.0) as i32 % 3 {
                    0 => Vec3::new(0.1, 1.0, 0.3),
                    1 => Vec3::new(1.0, 0.6, 0.1),
                    _ => Vec3::new(0.2, 0.5, 1.0),
                };
            }
        }
    }
    let holes = smoothstep(0.55, 0.6, ((bay * 90.0).sin() * (v * 220.0).sin()).abs()) * (bay > 0.5) as i32 as f32;
    let col = Vec3::splat(0.05 + frame * 0.07 + unit_line * 0.03 - holes * 0.03) + emit * 0.3;
    Px { col, h: 0.5 + frame * 0.4 - unit_line * 0.3 - holes * 0.3, rough: 0.35, metal: 0.7, emit, alpha: 1.0 }
}

fn tex_trim(u: f32, v: f32) -> Px {
    let n = value_noise(u * 64.0, v * 4.0, 64, 191);
    Px::new(Vec3::splat(0.06 + n * 0.02), 0.5 + n * 0.1, 0.7, 0.0)
}

fn tex_mud(u: f32, v: f32) -> Px {
    let n = fbm(u, v, 4, 5, 201);
    Px::new(Vec3::new(0.07, 0.065, 0.05) * (0.7 + n * 0.6), n, 0.6, 0.0)
}

fn tex_door(u: f32, v: f32) -> Px {
    // applied with explicit uv over a 2m x 3m slab
    let border = 1.0 - smoothstep(0.0, 0.02, edge_dist(u, v));
    let inset = ((u - 0.5).abs() < 0.42 && (v - 0.5).abs() < 0.44) as i32 as f32;
    let seam = 1.0 - smoothstep(0.0, 0.004, (u - 0.5).abs());
    let stripe = ((v - 0.52).abs() < 0.035) as i32 as f32;
    let brush = aniso(u, v, 2, 120, 2, 211);
    let base = Vec3::splat(0.32 + brush * 0.06) * (1.0 - inset * 0.12);
    let col = base.lerp(Vec3::splat(0.08), seam.max(border * 0.6)).lerp(Vec3::new(0.62, 0.62, 0.6), stripe * 0.8);
    Px::new(col, 0.6 - inset * 0.2 - seam * 0.5 - border * 0.3 + stripe * 0.1, 0.35 + brush * 0.1, 0.85)
}

fn tex_door_sec(u: f32, v: f32) -> Px {
    let band = v > 0.86 || v < 0.1;
    let hazard = if band { ((u * 10.0 + v * 6.0).fract() < 0.5) as i32 as f32 } else { 0.0 };
    let rivet = {
        let fu = (u * 8.0).fract();
        let fv = (v * 12.0).fract();
        1.0 - smoothstep(0.08, 0.14, ((fu - 0.5).powi(2) + (fv - 0.5).powi(2)).sqrt())
    };
    let seam = 1.0 - smoothstep(0.0, 0.004, (u - 0.5).abs());
    let wear = smoothstep(0.55, 0.85, fbm(u, v, 4, 4, 221));
    let mut col = Vec3::splat(0.24) * (1.0 - wear * 0.3);
    if band {
        col = if hazard > 0.5 { Vec3::new(0.75, 0.55, 0.05) } else { Vec3::splat(0.03) };
        col = col.lerp(Vec3::splat(0.2), wear * 0.5);
    }
    col = col.lerp(Vec3::splat(0.05), seam);
    Px::new(col, 0.5 + rivet * 0.4 - seam * 0.5, 0.45 + wear * 0.3, if band { 0.1 } else { 0.8 })
}

fn tex_grate(u: f32, v: f32) -> Px {
    let slat = (v * 10.0).fract();
    let open = slat > 0.35 && slat < 0.85 && u > 0.05 && u < 0.95 && v > 0.05 && v < 0.95;
    let col = if open { Vec3::ZERO } else { Vec3::splat(0.42) };
    Px { col, h: if open { 0.0 } else { 0.8 }, rough: 0.5, metal: 0.8, emit: Vec3::ZERO, alpha: if open { 0.0 } else { 1.0 } }
}

fn tex_water(u: f32, v: f32) -> Px {
    let n = fbm(u, v, 6, 5, 231);
    let n2 = fbm(u + 0.3, v, 12, 3, 232);
    Px::new(Vec3::new(0.012, 0.02, 0.024), n * 0.6 + n2 * 0.4, 0.03, 0.0)
}

pub fn surf_name(s: Surf) -> String {
    match s {
        Surf::Container(v) => format!("container{v}"),
        other => format!("{other:?}").to_lowercase(),
    }
}

// ------------------------------------------------------------------ library

#[derive(Resource)]
pub struct Mats {
    pub surf: HashMap<Surf, Handle<StandardMaterial>>,
    pub glass: Handle<StandardMaterial>,
    pub water: Handle<StandardMaterial>,
    pub door: Handle<StandardMaterial>,
    pub door_sec: Handle<StandardMaterial>,
    pub grate: Handle<StandardMaterial>,
    pub lamp_on: Handle<StandardMaterial>,
    pub lamp_warm: Handle<StandardMaterial>,
    pub lamp_off: Handle<StandardMaterial>,
    pub alarm_on: Handle<StandardMaterial>,
    pub screen: Handle<StandardMaterial>,
    pub screen_sec: Handle<StandardMaterial>,
    pub screen_off: Handle<StandardMaterial>,
    pub gunmetal: Handle<StandardMaterial>,
    pub gun_dark: Handle<StandardMaterial>,
    pub plastic: Handle<StandardMaterial>,
    pub metal: Handle<StandardMaterial>,
    pub black: Handle<StandardMaterial>,
    pub guard_armor: Handle<StandardMaterial>,
    pub guard_cloth: Handle<StandardMaterial>,
    pub guard_visor: Handle<StandardMaterial>,
    pub skin: Handle<StandardMaterial>,
    pub coat: Handle<StandardMaterial>,
    pub coat_accent: Handle<StandardMaterial>,
    pub shades: Handle<StandardMaterial>,
    pub npc_jacket: Handle<StandardMaterial>,
    pub npc_pants: Handle<StandardMaterial>,
    pub cloak: Handle<StandardMaterial>,
    pub led_green: Handle<StandardMaterial>,
    pub led_red: Handle<StandardMaterial>,
    pub led_amber: Handle<StandardMaterial>,
    pub led_off: Handle<StandardMaterial>,
    pub core_glow: Handle<StandardMaterial>,
    pub white: Handle<StandardMaterial>,
    pub red: Handle<StandardMaterial>,
    pub cyan_glow: Handle<StandardMaterial>,
    pub orange_glow: Handle<StandardMaterial>,
    pub yellow: Handle<StandardMaterial>,
    pub wood: Handle<StandardMaterial>,
    pub rain: Handle<StandardMaterial>,
    pub spark: Handle<StandardMaterial>,
    pub tracer: Handle<StandardMaterial>,
    pub muzzle: Handle<StandardMaterial>,
    pub blood: Handle<StandardMaterial>,
    pub decal: Handle<StandardMaterial>,
    pub holo: Handle<StandardMaterial>,
    pub beam: Handle<StandardMaterial>,
    pub glass_shard: Handle<StandardMaterial>,
    /// team match: [orange, blue] armour paint, dim emissive marker, holo floor zone
    pub team_paint: [Handle<StandardMaterial>; 2],
    pub team_glow: [Handle<StandardMaterial>; 2],
    pub team_holo: [Handle<StandardMaterial>; 2],
    pub team_screen: [Handle<StandardMaterial>; 2],
    pub hair: Handle<StandardMaterial>,
}

impl Mats {
    pub fn surf(&self, s: Surf) -> Handle<StandardMaterial> {
        self.surf.get(&s).cloned().unwrap_or_else(|| self.metal.clone())
    }
}

#[derive(Resource)]
pub struct Shapes {
    pub cube: Handle<Mesh>,
    pub sphere: Handle<Mesh>,
    pub sphere_lo: Handle<Mesh>,
    pub cylinder: Handle<Mesh>,
    pub capsule: Handle<Mesh>,
    pub quad: Handle<Mesh>,
    pub quad_v: Handle<Mesh>,
}

struct Recipe {
    surf: Option<Surf>,
    key: &'static str,
    size: u32,
    nstr: f32,
    f: fn(f32, f32) -> Px,
}

pub fn setup_materials(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    assets: Res<AssetServer>,
) {
    let t0 = std::time::Instant::now();
    let recipes: Vec<Recipe> = vec![
        Recipe { surf: Some(Surf::Concrete), key: "", size: 512, nstr: 3.0, f: tex_concrete },
        Recipe { surf: Some(Surf::Asphalt), key: "", size: 1024, nstr: 1.1, f: tex_asphalt },
        Recipe { surf: Some(Surf::FloorTile), key: "", size: 512, nstr: 4.0, f: tex_floor_tile },
        Recipe { surf: Some(Surf::Carpet), key: "", size: 512, nstr: 2.0, f: tex_carpet },
        Recipe { surf: Some(Surf::MetalFloor), key: "", size: 512, nstr: 6.0, f: tex_metal_floor },
        Recipe { surf: Some(Surf::WallLab), key: "", size: 512, nstr: 4.0, f: tex_wall_lab },
        Recipe { surf: Some(Surf::WallOffice), key: "", size: 512, nstr: 3.0, f: tex_wall_office },
        Recipe { surf: Some(Surf::WallMetal), key: "", size: 512, nstr: 5.0, f: tex_wall_metal },
        Recipe { surf: Some(Surf::WallExt), key: "", size: 1024, nstr: 4.0, f: tex_brick },
        Recipe { surf: Some(Surf::WallBoundary), key: "", size: 512, nstr: 3.0, f: tex_boundary },
        Recipe { surf: Some(Surf::Ceiling), key: "", size: 512, nstr: 3.0, f: tex_ceiling },
        Recipe { surf: Some(Surf::CeilingMetal), key: "", size: 256, nstr: 4.0, f: tex_ceiling_metal },
        Recipe { surf: Some(Surf::Roof), key: "", size: 256, nstr: 2.0, f: tex_roof },
        Recipe { surf: Some(Surf::Vent), key: "", size: 512, nstr: 4.0, f: tex_vent },
        Recipe { surf: Some(Surf::Crate), key: "", size: 512, nstr: 4.0, f: tex_crate },
        Recipe { surf: Some(Surf::Container(0)), key: "", size: 512, nstr: 5.0, f: tex_container },
        Recipe { surf: Some(Surf::Counter), key: "", size: 256, nstr: 2.0, f: tex_counter },
        Recipe { surf: Some(Surf::Rack), key: "", size: 512, nstr: 3.0, f: tex_rack },
        Recipe { surf: Some(Surf::Trim), key: "", size: 128, nstr: 1.0, f: tex_trim },
        Recipe { surf: Some(Surf::WaterBed), key: "", size: 256, nstr: 2.0, f: tex_mud },
        Recipe { surf: None, key: "door", size: 512, nstr: 4.0, f: tex_door },
        Recipe { surf: None, key: "door_sec", size: 512, nstr: 4.0, f: tex_door_sec },
        Recipe { surf: None, key: "grate", size: 256, nstr: 3.0, f: tex_grate },
        Recipe { surf: None, key: "water", size: 512, nstr: 2.5, f: tex_water },
    ];
    let outs: Vec<TexOut> = std::thread::scope(|s| {
        let handles: Vec<_> = recipes.iter().map(|r| s.spawn(move || generate(r.size, r.nstr, &r.f))).collect();
        handles.into_iter().map(|h| h.join().expect("texture thread")).collect()
    });
    info!("generated {} procedural materials in {:.2}s", outs.len(), t0.elapsed().as_secs_f32());

    let mut by_key: HashMap<String, (Handle<Image>, Handle<Image>, Handle<Image>, Option<Handle<Image>>)> = HashMap::new();
    for (r, o) in recipes.iter().zip(outs) {
        let name = r.surf.map(surf_name).unwrap_or_else(|| r.key.to_string());
        let name = if r.surf == Some(Surf::Container(0)) { "container".to_string() } else { name };
        let override_path = format!("assets/textures/{name}.png");
        let albedo = if std::path::Path::new(&override_path).exists() {
            info!("texture override: {override_path}");
            assets.load_with_settings::<Image, ImageLoaderSettings>(format!("textures/{name}.png"), |s: &mut ImageLoaderSettings| {
                s.sampler = ImageSampler::Descriptor(repeat_sampler());
            })
        } else {
            images.add(make_image(o.size, o.albedo, true))
        };
        let nrm_path = format!("assets/textures/{name}_normal.png");
        let normal = if std::path::Path::new(&nrm_path).exists() {
            assets.load_with_settings::<Image, ImageLoaderSettings>(format!("textures/{name}_normal.png"), |s: &mut ImageLoaderSettings| {
                s.sampler = ImageSampler::Descriptor(repeat_sampler());
                s.is_srgb = false;
            })
        } else {
            images.add(make_image(o.size, o.normal, false))
        };
        let mr = images.add(make_image(o.size, o.mr, false));
        let em = o.emissive.map(|e| images.add(make_image(o.size, e, true)));
        by_key.insert(name, (albedo, normal, mr, em));
    }

    let textured = |mats: &mut Assets<StandardMaterial>, key: &str, tint: Color| -> Handle<StandardMaterial> {
        let (a, n, mr, e) = by_key.get(key).cloned().expect("texture key");
        let has_e = e.is_some();
        mats.add(StandardMaterial {
            base_color: tint,
            base_color_texture: Some(a),
            normal_map_texture: Some(n),
            metallic_roughness_texture: Some(mr),
            metallic: 1.0,
            perceptual_roughness: 1.0,
            emissive: if has_e { emis(1.0, 1.0, 1.0, 120.0) } else { LinearRgba::BLACK },
            emissive_texture: e,
            ..default()
        })
    };

    let mut surf = HashMap::new();
    for s in [
        Surf::Concrete,
        Surf::Asphalt,
        Surf::FloorTile,
        Surf::Carpet,
        Surf::MetalFloor,
        Surf::WallLab,
        Surf::WallOffice,
        Surf::WallMetal,
        Surf::WallExt,
        Surf::WallBoundary,
        Surf::Ceiling,
        Surf::CeilingMetal,
        Surf::Roof,
        Surf::Vent,
        Surf::Crate,
        Surf::Counter,
        Surf::Rack,
        Surf::Trim,
        Surf::WaterBed,
    ] {
        surf.insert(s, textured(&mut mats, &surf_name(s), Color::WHITE));
    }
    let container_tints = [
        Color::srgb(0.62, 0.16, 0.10),
        Color::srgb(0.14, 0.28, 0.48),
        Color::srgb(0.20, 0.38, 0.22),
        Color::srgb(0.70, 0.50, 0.12),
        Color::srgb(0.38, 0.36, 0.33),
    ];
    for (i, t) in container_tints.iter().enumerate() {
        surf.insert(Surf::Container(i as u8), textured(&mut mats, "container", *t));
    }

    let mut col = |c: Color, rough: f32, metal: f32| {
        mats.add(StandardMaterial { base_color: c, perceptual_roughness: rough, metallic: metal, ..default() })
    };
    let gunmetal = col(Color::srgb(0.16, 0.17, 0.18), 0.35, 0.9);
    let gun_dark = col(Color::srgb(0.05, 0.05, 0.055), 0.5, 0.6);
    let plastic = col(Color::srgb(0.08, 0.08, 0.09), 0.6, 0.0);
    let metal = col(Color::srgb(0.55, 0.56, 0.58), 0.3, 1.0);
    let black = col(Color::srgb(0.02, 0.02, 0.02), 0.5, 0.0);
    let guard_armor = col(Color::srgb(0.10, 0.11, 0.13), 0.45, 0.4);
    let guard_cloth = col(Color::srgb(0.06, 0.065, 0.075), 0.85, 0.0);
    let skin = col(Color::srgb(0.55, 0.40, 0.32), 0.6, 0.0);
    let coat = col(Color::srgb(0.035, 0.035, 0.04), 0.7, 0.0);
    let npc_jacket = col(Color::srgb(0.28, 0.20, 0.12), 0.8, 0.0);
    let npc_pants = col(Color::srgb(0.12, 0.12, 0.14), 0.9, 0.0);
    let white = col(Color::srgb(0.85, 0.85, 0.82), 0.4, 0.0);
    let red = col(Color::srgb(0.7, 0.05, 0.04), 0.5, 0.0);
    let yellow = col(Color::srgb(0.8, 0.6, 0.05), 0.5, 0.0);
    let wood = col(Color::srgb(0.30, 0.20, 0.11), 0.7, 0.0);
    let shades = col(Color::srgb(0.01, 0.01, 0.012), 0.05, 0.9);
    let hair = col(Color::srgb(0.03, 0.025, 0.02), 0.8, 0.0);
    let team_paint = [col(Color::srgb(0.55, 0.22, 0.04), 0.5, 0.2), col(Color::srgb(0.06, 0.2, 0.5), 0.5, 0.2)];

    let mut glow = |base: Color, e: LinearRgba| mats.add(StandardMaterial { base_color: base, emissive: e, perceptual_roughness: 0.4, ..default() });
    let lamp_on = glow(Color::WHITE, emis(1.0, 0.97, 0.9, 900.0));
    let lamp_warm = glow(Color::WHITE, emis(1.0, 0.75, 0.45, 1000.0));
    let lamp_off = glow(Color::srgb(0.3, 0.3, 0.3), LinearRgba::BLACK);
    let alarm_on = glow(Color::srgb(0.6, 0.05, 0.05), emis(1.0, 0.05, 0.03, 1200.0));
    let screen = glow(Color::BLACK, emis(0.25, 0.85, 0.75, 45.0));
    let screen_sec = glow(Color::BLACK, emis(0.95, 0.55, 0.12, 45.0));
    let screen_off = glow(Color::srgb(0.02, 0.02, 0.025), LinearRgba::BLACK);
    let guard_visor = glow(Color::BLACK, emis(1.0, 0.18, 0.05, 500.0));
    let coat_accent = glow(Color::BLACK, emis(0.1, 0.8, 1.0, 300.0));
    let led_green = glow(Color::BLACK, emis(0.1, 1.0, 0.3, 600.0));
    let led_red = glow(Color::BLACK, emis(1.0, 0.06, 0.04, 800.0));
    let led_amber = glow(Color::BLACK, emis(1.0, 0.6, 0.1, 600.0));
    let led_off = glow(Color::srgb(0.05, 0.05, 0.05), LinearRgba::BLACK);
    let core_glow = glow(Color::BLACK, emis(0.2, 0.9, 1.0, 900.0));
    let cyan_glow = glow(Color::BLACK, emis(0.2, 0.8, 1.0, 220.0));
    let orange_glow = glow(Color::BLACK, emis(1.0, 0.5, 0.1, 400.0));
    // dim on purpose: team markers must not turn players into beacons in the dark
    let team_glow = [glow(Color::BLACK, emis(1.0, 0.45, 0.08, 40.0)), glow(Color::BLACK, emis(0.2, 0.5, 1.0, 40.0))];
    let team_screen = [glow(Color::BLACK, emis(1.0, 0.5, 0.12, 50.0)), glow(Color::BLACK, emis(0.25, 0.55, 1.0, 50.0))];

    let rain = mats.add(StandardMaterial {
        base_color: Color::srgba(0.75, 0.8, 0.9, 0.22),
        emissive: emis(0.5, 0.6, 0.75, 4.0),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.15,
        reflectance: 0.8,
        ..default()
    });
    let mut fx = |e: LinearRgba, alpha: f32, mode: AlphaMode| {
        mats.add(StandardMaterial {
            base_color: Color::srgba(0.0, 0.0, 0.0, alpha),
            emissive: e,
            alpha_mode: mode,
            unlit: false,
            cull_mode: None,
            perceptual_roughness: 1.0,
            ..default()
        })
    };
    let spark = fx(emis(1.0, 0.7, 0.3, 2500.0), 1.0, AlphaMode::Add);
    let tracer = fx(emis(1.0, 0.8, 0.45, 1200.0), 0.7, AlphaMode::Add);
    let muzzle = fx(emis(1.0, 0.75, 0.35, 3000.0), 1.0, AlphaMode::Add);
    let holo = fx(emis(0.2, 0.8, 1.0, 45.0), 0.3, AlphaMode::Add);
    let beam = fx(emis(0.2, 0.9, 1.0, 14.0), 0.15, AlphaMode::Add);
    let team_holo = [fx(emis(1.0, 0.5, 0.1, 30.0), 0.25, AlphaMode::Add), fx(emis(0.2, 0.55, 1.0, 30.0), 0.25, AlphaMode::Add)];
    let blood = mats.add(StandardMaterial { base_color: Color::srgb(0.25, 0.0, 0.0), perceptual_roughness: 0.3, ..default() });
    let decal = mats.add(StandardMaterial {
        base_color: Color::srgba(0.02, 0.02, 0.02, 0.85),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.9,
        ..default()
    });
    let glass_shard = mats.add(StandardMaterial {
        base_color: Color::srgba(0.7, 0.8, 0.85, 0.4),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.05,
        ..default()
    });
    let cloak = mats.add(StandardMaterial {
        base_color: Color::srgba(0.3, 0.5, 0.7, 0.08),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.05,
        emissive: emis(0.1, 0.3, 0.5, 8.0),
        ..default()
    });

    let glass = mats.add(StandardMaterial {
        base_color: Color::srgba(0.55, 0.65, 0.7, 0.12),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.04,
        reflectance: 0.6,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    let (wa, wn, _, _) = by_key.get("water").cloned().unwrap();
    let water = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(wa),
        normal_map_texture: Some(wn),
        perceptual_roughness: 0.04,
        metallic: 0.0,
        reflectance: 0.7,
        ..default()
    });
    let door = textured(&mut mats, "door", Color::WHITE);
    let door_sec = textured(&mut mats, "door_sec", Color::WHITE);
    let (ga, gn, gmr, _) = by_key.get("grate").cloned().unwrap();
    let grate = mats.add(StandardMaterial {
        base_color_texture: Some(ga),
        normal_map_texture: Some(gn),
        metallic_roughness_texture: Some(gmr),
        metallic: 1.0,
        perceptual_roughness: 1.0,
        alpha_mode: AlphaMode::Mask(0.5),
        cull_mode: None,
        double_sided: true,
        ..default()
    });

    commands.insert_resource(Mats {
        surf,
        glass,
        water,
        door,
        door_sec,
        grate,
        lamp_on,
        lamp_warm,
        lamp_off,
        alarm_on,
        screen,
        screen_sec,
        screen_off,
        gunmetal,
        gun_dark,
        plastic,
        metal,
        black,
        guard_armor,
        guard_cloth,
        guard_visor,
        skin,
        coat,
        coat_accent,
        shades,
        npc_jacket,
        npc_pants,
        cloak,
        led_green,
        led_red,
        led_amber,
        led_off,
        core_glow,
        white,
        red,
        cyan_glow,
        orange_glow,
        yellow,
        wood,
        rain,
        spark,
        tracer,
        muzzle,
        blood,
        decal,
        holo,
        beam,
        glass_shard,
        team_paint,
        team_glow,
        team_holo,
        team_screen,
        hair,
    });

    commands.insert_resource(Shapes {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        sphere: meshes.add(Sphere::new(0.5).mesh().uv(24, 16)),
        sphere_lo: meshes.add(Sphere::new(0.5).mesh().ico(1).unwrap()),
        cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
        capsule: meshes.add(Capsule3d::new(0.5, 1.0)),
        quad: meshes.add(Plane3d::default().mesh().size(1.0, 1.0)),
        quad_v: meshes.add(Rectangle::new(1.0, 1.0)),
    });
}
