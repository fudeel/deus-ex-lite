//! Unreal Engine 1 `Texture` objects -> RGBA8 pixels.
use super::package::*;

/// Decoded texture: top mip level as RGBA8 (plus the smaller mips Unreal stored, if wanted).
pub struct TexImage {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// RGBA8, row major
    pub rgba: Vec<u8>,
    /// palette index 0 is transparent (masked textures: grates, fences, foliage)
    pub masked: bool,
}

struct Mip {
    start: usize,
    len: usize,
    w: u32,
    h: u32,
}

const TEXF_P8: u8 = 0;
const TEXF_DXT1: u8 = 3;
const TEXF_RGB8: u8 = 4;
const TEXF_RGBA8: u8 = 5;

fn read_mips(pkg: &Package, r: &mut Reader) -> Result<Vec<Mip>> {
    let n = r.count(1)?;
    let mut mips = Vec::with_capacity(n);
    for _ in 0..n.min(16) {
        let len = pkg.lazy_count(r, 1)?;
        let start = r.pos;
        r.bytes(len)?;
        let w = r.i32()?.max(0) as u32;
        let h = r.i32()?.max(0) as u32;
        let _ubits = r.u8()?;
        let _vbits = r.u8()?;
        mips.push(Mip { start, len, w, h });
    }
    Ok(mips)
}

/// 256 RGBA colours of a `Palette` export.
pub fn read_palette(pkg: &Package, export: usize) -> Result<Vec<[u8; 4]>> {
    let mut r = pkg.reader(export);
    pkg.read_props(&mut r)?;
    let n = r.count(4)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let b = r.bytes(4)?;
        out.push([b[0], b[1], b[2], b[3]]);
    }
    Ok(out)
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [((r * 255 + 15) / 31) as u8, ((g * 255 + 31) / 63) as u8, ((b * 255 + 15) / 31) as u8]
}

fn decode_dxt1(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    let bw = w.div_ceil(4);
    let bh = h.div_ceil(4);
    for by in 0..bh {
        for bx in 0..bw {
            let o = ((by * bw + bx) * 8) as usize;
            if o + 8 > src.len() {
                return out;
            }
            let c0 = u16::from_le_bytes([src[o], src[o + 1]]);
            let c1 = u16::from_le_bytes([src[o + 2], src[o + 3]]);
            let bits = u32::from_le_bytes([src[o + 4], src[o + 5], src[o + 6], src[o + 7]]);
            let a = rgb565(c0);
            let b = rgb565(c1);
            let mut pal = [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], [0; 4], [0; 4]];
            if c0 > c1 {
                for k in 0..3 {
                    pal[2][k] = ((2 * a[k] as u32 + b[k] as u32) / 3) as u8;
                    pal[3][k] = ((a[k] as u32 + 2 * b[k] as u32) / 3) as u8;
                }
                pal[2][3] = 255;
                pal[3][3] = 255;
            } else {
                for k in 0..3 {
                    pal[2][k] = ((a[k] as u32 + b[k] as u32) / 2) as u8;
                }
                pal[2][3] = 255;
                pal[3] = [0, 0, 0, 0];
            }
            for py in 0..4 {
                for px in 0..4 {
                    let (x, y) = (bx * 4 + px, by * 4 + py);
                    if x >= w || y >= h {
                        continue;
                    }
                    let idx = ((bits >> (2 * (py * 4 + px))) & 3) as usize;
                    let d = ((y * w + x) * 4) as usize;
                    out[d..d + 4].copy_from_slice(&pal[idx]);
                }
            }
        }
    }
    out
}

/// Decode a `Texture` export to RGBA8 (top mip).
pub fn read_texture(pkg: &Package, export: usize) -> Result<TexImage> {
    let name = pkg.name(pkg.exports[export].name).to_string();
    let mut r = pkg.reader(export);
    let props = pkg.read_props(&mut r)?;
    let format = find_prop(&props, "Format").map(|p| pkg.prop_u8(p)).unwrap_or(TEXF_P8);
    let mut masked = find_prop(&props, "bMasked").map(|p| p.bool_value).unwrap_or(false);
    let palette_ref = find_prop(&props, "Palette").map(|p| pkg.prop_index(p)).unwrap_or(0);
    let mips = read_mips(pkg, &mut r)?;
    let Some(m) = mips.first() else { return Err(PkgError(format!("{name}: no mipmaps"))) };
    if m.w == 0 || m.h == 0 || m.w > 4096 || m.h > 4096 {
        return Err(PkgError(format!("{name}: bad size {}x{}", m.w, m.h)));
    }
    let src = &pkg.data[m.start..m.start + m.len];
    let n = (m.w * m.h) as usize;
    let rgba = match format {
        TEXF_P8 => {
            if palette_ref <= 0 {
                return Err(PkgError(format!("{name}: palette is not in this package")));
            }
            let pal = read_palette(pkg, palette_ref as usize - 1)?;
            if src.len() < n || pal.len() < 256 {
                return Err(PkgError(format!("{name}: truncated P8 data")));
            }
            // Unreal masks palette index 0 per *surface* (PF_Masked), so some see-through
            // textures (grates, fences) do not set bMasked themselves; their index 0 is the
            // conventional magenta key colour.
            let key = pal[0];
            if key[0] > 200 && key[1] < 60 && key[2] > 200 {
                masked = true;
            }
            let mut out = Vec::with_capacity(n * 4);
            for &i in &src[..n] {
                let c = pal[i as usize];
                let a = if masked && i == 0 { 0 } else { 255 };
                out.extend_from_slice(&[c[0], c[1], c[2], a]);
            }
            out
        }
        TEXF_DXT1 => decode_dxt1(src, m.w, m.h),
        TEXF_RGBA8 => {
            if src.len() < n * 4 {
                return Err(PkgError(format!("{name}: truncated RGBA8 data")));
            }
            // stored as BGRA
            let mut out = Vec::with_capacity(n * 4);
            for p in src[..n * 4].chunks(4) {
                out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
            }
            out
        }
        TEXF_RGB8 => {
            if src.len() < n * 3 {
                return Err(PkgError(format!("{name}: truncated RGB8 data")));
            }
            let mut out = Vec::with_capacity(n * 4);
            for p in src[..n * 3].chunks(3) {
                out.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
            out
        }
        f => return Err(PkgError(format!("{name}: unsupported texture format {f}"))),
    };
    let mut img = TexImage { name, width: m.w, height: m.h, rgba, masked };
    if img.masked || format == TEXF_DXT1 {
        img.bleed_transparent();
    }
    Ok(img)
}

impl TexImage {
    /// Give fully transparent pixels the colour of their opaque neighbours so bilinear
    /// filtering and mipmaps do not smear the magenta key colour into visible edges.
    pub fn bleed_transparent(&mut self) {
        let (w, h) = (self.width as i32, self.height as i32);
        if !self.rgba.chunks(4).any(|p| p[3] == 0) {
            return;
        }
        let mut known: Vec<bool> = self.rgba.chunks(4).map(|p| p[3] != 0).collect();
        if !known.iter().any(|k| *k) {
            for p in self.rgba.chunks_mut(4) {
                p[0] = 0;
                p[1] = 0;
                p[2] = 0;
            }
            return;
        }
        // a few dilation passes are enough for the mip chain; the rest gets the mean colour
        for _ in 0..8 {
            let mut next = known.clone();
            let mut changed = false;
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    if known[i] {
                        continue;
                    }
                    let mut acc = [0u32; 3];
                    let mut n = 0;
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                        let j = ((y + dy).rem_euclid(h) * w + (x + dx).rem_euclid(w)) as usize;
                        if known[j] {
                            for c in 0..3 {
                                acc[c] += self.rgba[j * 4 + c] as u32;
                            }
                            n += 1;
                        }
                    }
                    if n > 0 {
                        for c in 0..3 {
                            self.rgba[i * 4 + c] = (acc[c] / n) as u8;
                        }
                        next[i] = true;
                        changed = true;
                    }
                }
            }
            known = next;
            if !changed {
                break;
            }
        }
        let mut mean = [0u64; 3];
        let mut n = 0u64;
        for p in self.rgba.chunks(4).filter(|p| p[3] != 0) {
            for c in 0..3 {
                mean[c] += p[c] as u64;
            }
            n += 1;
        }
        for (i, p) in self.rgba.chunks_mut(4).enumerate() {
            if !known[i] && n > 0 {
                for c in 0..3 {
                    p[c] = (mean[c] / n) as u8;
                }
            }
        }
    }
}
