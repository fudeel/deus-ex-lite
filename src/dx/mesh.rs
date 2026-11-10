//! Unreal Engine 1 `LodMesh` (vertex-animated mesh) reader.
//!
//! Layout follows the engine's `UMesh` / `ULodMesh` serializers for package version 61-69.
//! Deus Ex stores vertices as four signed 16-bit values (X, Y, Z, pad) instead of the usual
//! packed 11:11:10 dword; both layouts are handled. Every animation frame is a full copy of
//! the vertex positions.
use super::package::*;

#[derive(Clone, Debug)]
pub struct AnimSeq {
    pub name: String,
    pub group: String,
    pub start: u32,
    pub frames: u32,
    pub rate: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Wedge {
    pub vertex: u16,
    /// texel coordinates in 0..255 of the material's texture
    pub u: u8,
    pub v: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Face {
    pub wedges: [u16; 3],
    pub material: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub flags: u32,
    pub texture: i32,
}

pub struct LodMesh {
    pub name: String,
    /// vertices per animation frame
    pub frame_verts: usize,
    pub frames: usize,
    /// raw mesh-space positions, `frames * frame_verts` entries
    pub verts: Vec<[f32; 3]>,
    pub wedges: Vec<Wedge>,
    pub faces: Vec<Face>,
    pub materials: Vec<Material>,
    /// object references of the default skins (index = material texture slot)
    pub textures: Vec<i32>,
    pub seqs: Vec<AnimSeq>,
    /// weapon attachment triangles (vertex indices into each frame); characters have one
    pub special_faces: Vec<[u16; 3]>,
    pub scale: [f32; 3],
    pub origin: [f32; 3],
    /// pitch, yaw, roll in Unreal units (65536 = full turn)
    pub rot_origin: [i32; 3],
}

/// Polygon flags of mesh materials.
pub const PF_TWO_SIDED: u32 = 0x0000_0100;
pub const PF_TRANSLUCENT: u32 = 0x0000_0004;
pub const PF_MASKED: u32 = 0x0000_0002;
pub const PF_MODULATED: u32 = 0x0000_0040;
pub const PF_UNLIT: u32 = 0x0040_0000;
pub const PF_INVISIBLE: u32 = 0x0000_0001;

fn read_box(r: &mut Reader) -> Result<()> {
    r.bytes(12 + 12 + 1)?;
    Ok(())
}

fn read_sphere(r: &mut Reader, version: u16) -> Result<()> {
    r.bytes(if version >= 61 { 16 } else { 12 })?;
    Ok(())
}

fn skip_lazy(pkg: &Package, r: &mut Reader, item: usize) -> Result<usize> {
    let n = pkg.lazy_count(r, item)?;
    r.bytes(n * item)?;
    Ok(n)
}

fn read_u16_array(r: &mut Reader) -> Result<Vec<u16>> {
    let n = r.count(2)?;
    (0..n).map(|_| r.u16()).collect()
}

pub fn read_lodmesh(pkg: &Package, export: usize) -> Result<LodMesh> {
    let name = pkg.name(pkg.exports[export].name).to_string();
    let class = pkg.class_of(&pkg.exports[export]).to_string();
    let is_lod = class.eq_ignore_ascii_case("LodMesh");
    if !is_lod && !class.eq_ignore_ascii_case("Mesh") {
        return Err(PkgError(format!("{name}: {class} is not a mesh")));
    }
    let ver = pkg.version;
    let mut r = pkg.reader(export);
    pkg.read_props(&mut r)?;
    // UPrimitive
    read_box(&mut r)?;
    read_sphere(&mut r, ver)?;

    // UMesh::Verts - lazy array; the item size tells the Deus Ex layout apart
    let mut deus = false;
    if ver > 61 {
        let save = r.pos;
        let skip = r.i32()? as usize;
        let n = r.index()?.max(0) as usize;
        if n > 0 && skip >= r.pos && skip - r.pos == n * 8 {
            deus = true;
        }
        r.seek(save);
    }
    let nv = pkg.lazy_count(&mut r, if deus { 8 } else { 4 })?;
    let mut verts = Vec::with_capacity(nv);
    for _ in 0..nv {
        if deus {
            let x = r.i16()? as f32;
            let y = r.i16()? as f32;
            let z = r.i16()? as f32;
            let _pad = r.i16()?;
            verts.push([x, y, z]);
        } else {
            let d = r.u32()?;
            let x = ((d << 21) as i32 >> 21) as f32;
            let y = ((d << 10) as i32 >> 21) as f32;
            let z = (d as i32 >> 22) as f32;
            verts.push([x, y, z]);
        }
    }
    // Tris (used only by plain `Mesh`)
    let ntris = pkg.lazy_count(&mut r, 16)?;
    let mut tris = Vec::with_capacity(ntris);
    for _ in 0..ntris {
        let iv = [r.u16()?, r.u16()?, r.u16()?];
        let uv = [(r.u8()?, r.u8()?), (r.u8()?, r.u8()?), (r.u8()?, r.u8()?)];
        let flags = r.u32()?;
        let tex = r.i32()?;
        tris.push((iv, uv, flags, tex));
    }
    // AnimSeqs
    let nseq = r.count(1)?;
    let mut seqs = Vec::with_capacity(nseq);
    for _ in 0..nseq {
        let sname = pkg.name(r.index()?).to_string();
        let group = pkg.name(r.index()?).to_string();
        let start = r.i32()?.max(0) as u32;
        let frames = r.i32()?.max(0) as u32;
        let nn = r.count(1)?;
        for _ in 0..nn {
            r.f32()?;
            r.index()?;
        }
        let rate = r.f32()?;
        seqs.push(AnimSeq { name: sname, group, start, frames, rate });
    }
    skip_lazy(pkg, &mut r, 8)?; // Connects
    read_box(&mut r)?;
    read_sphere(&mut r, ver)?;
    skip_lazy(pkg, &mut r, 4)?; // VertLinks
    let ntex = r.count(1)?;
    let textures: Vec<i32> = (0..ntex).map(|_| r.index()).collect::<Result<_>>()?;
    let nbb = r.count(25)?;
    for _ in 0..nbb {
        read_box(&mut r)?;
    }
    let nbs = r.count(12)?;
    for _ in 0..nbs {
        read_sphere(&mut r, ver)?;
    }
    let frame_verts = r.i32()?.max(0) as usize;
    let frames = r.i32()?.max(0) as usize;
    let _and = r.u32()?;
    let _or = r.u32()?;
    let scale = r.vec3()?;
    let origin = r.vec3()?;
    let rot_origin = [r.i32()?, r.i32()?, r.i32()?];
    let _cur_poly = r.i32()?;
    let _cur_vertex = r.i32()?;
    if ver == 65 {
        r.f32()?;
    } else if ver >= 66 {
        let n = r.count(4)?;
        r.bytes(n * 4)?;
    }

    let mut mesh = LodMesh {
        name: name.clone(),
        frame_verts,
        frames,
        verts,
        wedges: vec![],
        faces: vec![],
        materials: vec![],
        textures,
        seqs,
        special_faces: vec![],
        scale,
        origin,
        rot_origin,
    };

    if is_lod {
        let _collapse_points = read_u16_array(&mut r)?;
        let _face_level = read_u16_array(&mut r)?;
        let nf = r.count(8)?;
        for _ in 0..nf {
            let w = [r.u16()?, r.u16()?, r.u16()?];
            let m = r.u16()?;
            mesh.faces.push(Face { wedges: w, material: m });
        }
        let _collapse_wedges = read_u16_array(&mut r)?;
        let nw = r.count(4)?;
        for _ in 0..nw {
            let vertex = r.u16()?;
            let u = r.u8()?;
            let v = r.u8()?;
            mesh.wedges.push(Wedge { vertex, u, v });
        }
        let nm = r.count(8)?;
        for _ in 0..nm {
            let flags = r.u32()?;
            let texture = r.i32()?;
            mesh.materials.push(Material { flags, texture });
        }
        let nsf = r.count(8)?;
        for _ in 0..nsf {
            let w = [r.u16()?, r.u16()?, r.u16()?];
            let _m = r.u16()?;
            mesh.special_faces.push(w);
        }
        let _model_verts = r.i32()?;
        let special_verts = r.i32()?.max(0) as usize;
        for _ in 0..6 {
            r.u32()?; // MeshScaleMax, LODHysteresis, LODStrength, LODMinVerts, LODMorph, LODZDisplace
        }
        let remap = read_u16_array(&mut r)?;
        let old_frame_verts = r.i32()?.max(0) as usize;
        for w in &mut mesh.wedges {
            w.vertex = (w.vertex as usize + special_verts).min(u16::MAX as usize) as u16;
        }
        if !remap.is_empty() && old_frame_verts > 0 {
            let mut nv = Vec::with_capacity(frames * frame_verts);
            for f in 0..frames {
                for k in 0..frame_verts {
                    let src = old_frame_verts * f + *remap.get(k).unwrap_or(&0) as usize;
                    nv.push(*mesh.verts.get(src).unwrap_or(&[0.0; 3]));
                }
            }
            mesh.verts = nv;
        }
    } else {
        // plain UMesh: one wedge per triangle corner, one material per texture slot
        for (iv, uv, flags, tex) in tris {
            let t = tex.max(0) as usize;
            while mesh.materials.len() <= t {
                mesh.materials.push(Material { flags: 0, texture: mesh.materials.len() as i32 });
            }
            mesh.materials[t].flags |= flags;
            let base = mesh.wedges.len() as u16;
            for k in 0..3 {
                mesh.wedges.push(Wedge { vertex: iv[k], u: uv[k].0, v: uv[k].1 });
            }
            mesh.faces.push(Face { wedges: [base, base + 1, base + 2], material: t as u16 });
        }
    }

    if mesh.frame_verts == 0 || mesh.frames == 0 || mesh.verts.len() < mesh.frame_verts * mesh.frames {
        return Err(PkgError(format!(
            "{name}: inconsistent frame data ({} verts, {} x {})",
            mesh.verts.len(),
            mesh.frames,
            mesh.frame_verts
        )));
    }
    if let Some(bad) = mesh.wedges.iter().find(|w| w.vertex as usize >= mesh.frame_verts) {
        return Err(PkgError(format!("{name}: wedge vertex {} out of range", bad.vertex)));
    }
    if let Some(bad) = mesh.faces.iter().find(|f| f.wedges.iter().any(|w| *w as usize >= mesh.wedges.len())) {
        return Err(PkgError(format!("{name}: face wedge {:?} out of range", bad.wedges)));
    }
    Ok(mesh)
}

impl LodMesh {
    pub fn seq(&self, name: &str) -> Option<&AnimSeq> {
        self.seqs.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn frame(&self, f: usize) -> &[[f32; 3]] {
        let f = f.min(self.frames - 1);
        &self.verts[f * self.frame_verts..(f + 1) * self.frame_verts]
    }

    /// Mesh-space vertex -> game space in metres: Y up, the character facing -Z with its
    /// right (weapon) hand on +X. Unreal is left handed (X forward, Y right, Z up), so the
    /// stored triangle order is clockwise here: build faces as `[0, 2, 1]`.
    pub fn to_world(&self, v: [f32; 3], units_per_m: f32) -> [f32; 3] {
        let p = [(v[0] - self.origin[0]) * self.scale[0], (v[1] - self.origin[1]) * self.scale[1], (v[2] - self.origin[2]) * self.scale[2]];
        let yaw = self.rot_origin[1] as f32 * std::f32::consts::TAU / 65536.0;
        let (s, c) = yaw.sin_cos();
        let x = p[0] * c - p[1] * s;
        let y = p[0] * s + p[1] * c;
        let z = p[2];
        [y / units_per_m, z / units_per_m, -x / units_per_m]
    }
}
