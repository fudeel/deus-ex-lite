//! Unreal Engine 1 `Sound` objects -> PCM samples.
//!
//! A `Sound` export is its properties, the file-type name (always `WAV` in Deus Ex) and a lazy
//! byte array holding the original RIFF file. The game re-encodes what it plays as 16-bit
//! mono PCM so that every clip goes through the same decoder path, whatever bit depth,
//! channel count or chunk order the original file used.
use super::package::*;

pub struct Pcm {
    pub name: String,
    pub rate: u32,
    /// mono, -1..1
    pub samples: Vec<f32>,
    /// loop region from the `smpl` chunk, if any (start, end) in samples
    pub loop_range: Option<(u32, u32)>,
}

/// Raw bytes of the embedded sound file.
pub fn sound_bytes<'a>(pkg: &'a Package, export: usize) -> Result<(&'a str, &'a [u8])> {
    let mut r = pkg.reader(export);
    pkg.read_props(&mut r)?;
    let ftype = pkg.name(r.index()?);
    let len = pkg.lazy_count(&mut r, 1)?;
    let bytes = r.bytes(len)?;
    Ok((ftype, bytes))
}

fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Decode a RIFF/WAVE PCM file (8/16/24/32-bit integer or 32-bit float, any channel count).
pub fn decode_wav(name: &str, b: &[u8]) -> Result<Pcm> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(PkgError(format!("{name}: not a RIFF/WAVE file")));
    }
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut data: Option<&[u8]> = None;
    let mut loop_range = None;
    let mut o = 12;
    while o + 8 <= b.len() {
        let id = &b[o..o + 4];
        let size = le32(b, o + 4) as usize;
        let body_end = (o + 8 + size).min(b.len());
        let body = &b[o + 8..body_end];
        match id {
            b"fmt " if body.len() >= 16 => {
                fmt = Some((le16(body, 0), le16(body, 2), le32(body, 4), le16(body, 14)));
            }
            b"data" => data = Some(body),
            b"smpl" if body.len() >= 36 + 24 => {
                let loops = le32(body, 28);
                if loops > 0 {
                    loop_range = Some((le32(body, 36 + 8), le32(body, 36 + 12)));
                }
            }
            _ => {}
        }
        o += 8 + size + (size & 1);
    }
    let Some((tag, channels, rate, bits)) = fmt else { return Err(PkgError(format!("{name}: no fmt chunk"))) };
    let Some(data) = data else { return Err(PkgError(format!("{name}: no data chunk"))) };
    let ch = channels.max(1) as usize;
    let float = match tag {
        1 => false,
        3 => true,
        0xFFFE if bits == 32 => false,
        0xFFFE => false,
        t => return Err(PkgError(format!("{name}: unsupported WAV codec {t:#x}"))),
    };
    let bps = (bits as usize).div_ceil(8);
    if bps == 0 || bps > 4 {
        return Err(PkgError(format!("{name}: unsupported bit depth {bits}")));
    }
    let frame = bps * ch;
    let frames = data.len() / frame;
    let mut samples = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut acc = 0.0f32;
        for c in 0..ch {
            let s = &data[f * frame + c * bps..f * frame + (c + 1) * bps];
            let v = match (bps, float) {
                (1, _) => (s[0] as f32 - 128.0) / 128.0,
                (2, _) => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
                (3, _) => (((s[0] as i32) << 8 | (s[1] as i32) << 16 | (s[2] as i32) << 24) >> 8) as f32 / 8_388_608.0,
                (4, true) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                (4, false) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0,
                _ => 0.0,
            };
            acc += v;
        }
        samples.push((acc / ch as f32).clamp(-1.0, 1.0));
    }
    Ok(Pcm { name: name.to_string(), rate: rate.max(1), samples, loop_range })
}

/// Decode the `Sound` export at `export`.
pub fn read_sound(pkg: &Package, export: usize) -> Result<Pcm> {
    let name = pkg.name(pkg.exports[export].name).to_string();
    let (ftype, bytes) = sound_bytes(pkg, export)?;
    if !ftype.eq_ignore_ascii_case("wav") {
        return Err(PkgError(format!("{name}: unsupported sound type {ftype}")));
    }
    decode_wav(&name, bytes)
}

/// Find a sound by `Group.Name` (or just `Name`) and decode it.
pub fn find_sound(pkg: &Package, path: &str) -> Result<Pcm> {
    let (group, name) = match path.rsplit_once('.') {
        Some((g, n)) => (Some(g), n),
        None => (None, path),
    };
    let found = pkg
        .exports
        .iter()
        .enumerate()
        .find(|(_, e)| {
            pkg.class_of(e).eq_ignore_ascii_case("Sound")
                && pkg.name(e.name).eq_ignore_ascii_case(name)
                && group.map(|g| pkg.group_of(e).eq_ignore_ascii_case(g)).unwrap_or(true)
        })
        .map(|(i, _)| i);
    match found {
        Some(i) => read_sound(pkg, i),
        None => Err(PkgError(format!("sound {path} not found"))),
    }
}

impl Pcm {
    /// 16-bit mono PCM WAV file bytes.
    pub fn to_wav(&self, gain: f32) -> Vec<u8> {
        let n = self.samples.len();
        let mut out = Vec::with_capacity(44 + n * 2);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + n * 2) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&self.rate.to_le_bytes());
        out.extend_from_slice(&(self.rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&((n * 2) as u32).to_le_bytes());
        for s in &self.samples {
            out.extend_from_slice(&(((s * gain).clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        out
    }

    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / self.rate as f32
    }

    pub fn peak(&self) -> f32 {
        self.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }
}
