//! Reader for Unreal Engine 1 packages (`.u`, `.utx`, `.uax`, ...) as shipped with Deus Ex.
//!
//! Only what the game needs: the name / import / export tables, tagged object properties
//! and raw access to each export's serialized data. Std only, no allocations beyond the
//! file itself.
use std::fmt;

#[derive(Debug)]
pub struct PkgError(pub String);

impl fmt::Display for PkgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub type Result<T> = std::result::Result<T, PkgError>;

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(PkgError(msg.into()))
}

const TAG: u32 = 0x9E2A_83C1;

/// Little-endian cursor over a byte slice.
#[derive(Clone)]
pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], pos: usize) -> Self {
        Reader { data, pos }
    }
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.data.len() {
            return err(format!("read past end ({} + {} > {})", self.pos, n, self.data.len()));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    pub fn u32(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    pub fn vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
    /// Unreal "compact index": 1-5 bytes, sign bit in the first byte.
    pub fn index(&mut self) -> Result<i32> {
        let b0 = self.u8()?;
        let neg = b0 & 0x80 != 0;
        let mut v: i64 = (b0 & 0x3F) as i64;
        if b0 & 0x40 != 0 {
            let mut shift = 6;
            for _ in 0..4 {
                let b = self.u8()?;
                v |= ((b & 0x7F) as i64) << shift;
                shift += 7;
                if b & 0x80 == 0 {
                    break;
                }
            }
        }
        if v > i32::MAX as i64 {
            return err("compact index overflow");
        }
        Ok(if neg { -(v as i32) } else { v as i32 })
    }
    /// Array length (compact index), sanity checked against the remaining bytes.
    pub fn count(&mut self, item_size: usize) -> Result<usize> {
        let n = self.index()?;
        if n < 0 || (n as usize).saturating_mul(item_size.max(1)) > self.remaining() {
            return err(format!("bad array length {n} at {}", self.pos));
        }
        Ok(n as usize)
    }
    pub fn fstring(&mut self) -> Result<String> {
        let n = self.index()?;
        if n == 0 {
            return Ok(String::new());
        }
        if n > 0 {
            let b = self.bytes(n as usize)?;
            let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
            Ok(b[..end].iter().map(|&c| c as char).collect())
        } else {
            let n = (-n) as usize;
            let b = self.bytes(n * 2)?;
            let units: Vec<u16> = b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect();
            Ok(String::from_utf16_lossy(&units))
        }
    }
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }
}

#[derive(Clone, Debug)]
pub struct Import {
    pub class_package: i32,
    pub class_name: i32,
    pub package: i32,
    pub name: i32,
}

#[derive(Clone, Debug)]
pub struct Export {
    pub class: i32,
    pub super_: i32,
    pub package: i32,
    pub name: i32,
    pub flags: u32,
    pub size: usize,
    pub offset: usize,
}

pub struct Package {
    pub data: Vec<u8>,
    pub version: u16,
    pub licensee: u16,
    pub names: Vec<String>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

/// Unreal property types (low 4 bits of the tag info byte).
pub mod ptype {
    pub const BYTE: u8 = 1;
    pub const INT: u8 = 2;
    pub const BOOL: u8 = 3;
    pub const FLOAT: u8 = 4;
    pub const OBJECT: u8 = 5;
    pub const NAME: u8 = 6;
    pub const STRING: u8 = 7;
    pub const CLASS: u8 = 8;
    pub const ARRAY: u8 = 9;
    pub const STRUCT: u8 = 10;
    pub const VECTOR: u8 = 11;
    pub const ROTATOR: u8 = 12;
    pub const STR: u8 = 13;
    pub const MAP: u8 = 14;
    pub const FIXED_ARRAY: u8 = 15;
}

/// One tagged property of an object.
#[derive(Clone, Debug)]
pub struct Prop {
    pub name: String,
    pub kind: u8,
    pub struct_name: Option<String>,
    pub index: u32,
    pub bool_value: bool,
    /// absolute file range of the value
    pub start: usize,
    pub size: usize,
}

impl Package {
    pub fn load(path: &std::path::Path) -> Result<Package> {
        let data = std::fs::read(path).map_err(|e| PkgError(format!("{}: {e}", path.display())))?;
        Package::parse(data)
    }

    pub fn parse(data: Vec<u8>) -> Result<Package> {
        let mut r = Reader::new(&data, 0);
        if r.u32()? != TAG {
            return err("not an Unreal package");
        }
        let version = r.u16()?;
        let licensee = r.u16()?;
        let _flags = r.u32()?;
        let name_count = r.i32()?.max(0) as usize;
        let name_offset = r.i32()?.max(0) as usize;
        let export_count = r.i32()?.max(0) as usize;
        let export_offset = r.i32()?.max(0) as usize;
        let import_count = r.i32()?.max(0) as usize;
        let import_offset = r.i32()?.max(0) as usize;
        if version < 61 || version > 70 {
            return err(format!("unsupported package version {version}"));
        }

        let mut names = Vec::with_capacity(name_count);
        r.seek(name_offset);
        for _ in 0..name_count {
            let s = if version < 64 {
                let mut s = String::new();
                loop {
                    let c = r.u8()?;
                    if c == 0 {
                        break;
                    }
                    s.push(c as char);
                }
                s
            } else {
                r.fstring()?
            };
            let _flags = r.u32()?;
            names.push(s);
        }

        let mut imports = Vec::with_capacity(import_count);
        r.seek(import_offset);
        for _ in 0..import_count {
            let class_package = r.index()?;
            let class_name = r.index()?;
            let package = r.i32()?;
            let name = r.index()?;
            imports.push(Import { class_package, class_name, package, name });
        }

        let mut exports = Vec::with_capacity(export_count);
        r.seek(export_offset);
        for _ in 0..export_count {
            let class = r.index()?;
            let super_ = r.index()?;
            let package = r.i32()?;
            let name = r.index()?;
            let flags = r.u32()?;
            let size = r.index()?.max(0) as usize;
            let offset = if size > 0 { r.index()?.max(0) as usize } else { 0 };
            if offset + size > data.len() {
                return err("export outside the file");
            }
            exports.push(Export { class, super_, package, name, flags, size, offset });
        }
        Ok(Package { data, version, licensee, names, imports, exports })
    }

    pub fn name(&self, i: i32) -> &str {
        self.names.get(i.max(0) as usize).map(|s| s.as_str()).unwrap_or("?")
    }

    /// Name of an object reference (positive = export + 1, negative = -(import + 1)).
    pub fn object_name(&self, r: i32) -> &str {
        if r > 0 {
            self.exports.get(r as usize - 1).map(|e| self.name(e.name)).unwrap_or("?")
        } else if r < 0 {
            self.imports.get((-r) as usize - 1).map(|i| self.name(i.name)).unwrap_or("?")
        } else {
            "None"
        }
    }

    /// Class name of an export ("Texture", "Sound", "LodMesh"...).
    pub fn class_of(&self, e: &Export) -> &str {
        if e.class == 0 {
            "Class"
        } else {
            self.object_name(e.class)
        }
    }

    /// Group / package path of an export ("Skins", "Generic"...), outermost last.
    pub fn group_of(&self, e: &Export) -> String {
        let mut parts = vec![];
        let mut p = e.package;
        let mut guard = 0;
        while p > 0 && guard < 16 {
            let Some(x) = self.exports.get(p as usize - 1) else { break };
            parts.push(self.name(x.name).to_string());
            p = x.package;
            guard += 1;
        }
        parts.join(".")
    }

    pub fn find(&self, name: &str, class: &str) -> Option<usize> {
        self.exports.iter().position(|e| self.name(e.name).eq_ignore_ascii_case(name) && self.class_of(e).eq_ignore_ascii_case(class))
    }

    pub fn exports_of_class<'a>(&'a self, class: &'a str) -> impl Iterator<Item = (usize, &'a Export)> + 'a {
        self.exports.iter().enumerate().filter(move |(_, e)| self.class_of(e).eq_ignore_ascii_case(class))
    }

    pub fn export_data(&self, i: usize) -> &[u8] {
        let e = &self.exports[i];
        &self.data[e.offset..e.offset + e.size]
    }

    /// Reader positioned at the start of an export's data.
    pub fn reader(&self, i: usize) -> Reader<'_> {
        let e = &self.exports[i];
        Reader::new(&self.data[..e.offset + e.size], e.offset)
    }

    /// Read the tagged property list at the reader's position (leaves the reader after "None").
    pub fn read_props(&self, r: &mut Reader) -> Result<Vec<Prop>> {
        let mut out = vec![];
        for _ in 0..512 {
            let name_i = r.index()?;
            let name = self.name(name_i).to_string();
            if name.eq_ignore_ascii_case("None") {
                return Ok(out);
            }
            let info = r.u8()?;
            let kind = info & 0x0F;
            let array_bit = info & 0x80 != 0;
            let struct_name = if kind == ptype::STRUCT { Some(self.name(r.index()?).to_string()) } else { None };
            let size = match (info >> 4) & 7 {
                0 => 1,
                1 => 2,
                2 => 4,
                3 => 12,
                4 => 16,
                5 => r.u8()? as usize,
                6 => r.u16()? as usize,
                _ => r.u32()? as usize,
            };
            let mut index = 0u32;
            if array_bit && kind != ptype::BOOL {
                let b = r.u8()?;
                index = if b & 0x80 == 0 {
                    b as u32
                } else if b & 0xC0 == 0x80 {
                    (((b & 0x7F) as u32) << 8) | r.u8()? as u32
                } else {
                    let rest = r.bytes(3)?;
                    (((b & 0x3F) as u32) << 24) | ((rest[0] as u32) << 16) | ((rest[1] as u32) << 8) | rest[2] as u32
                };
            }
            let (size, bool_value) = if kind == ptype::BOOL { (0, array_bit) } else { (size, false) };
            let start = r.pos;
            r.bytes(size)?;
            out.push(Prop { name, kind, struct_name, index, bool_value, start, size });
        }
        err("too many properties")
    }

    /// Value helpers for properties read with `read_props`.
    pub fn prop_u8(&self, p: &Prop) -> u8 {
        self.data.get(p.start).copied().unwrap_or(0)
    }
    pub fn prop_i32(&self, p: &Prop) -> i32 {
        if p.size < 4 {
            return 0;
        }
        let b = &self.data[p.start..p.start + 4];
        i32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }
    pub fn prop_f32(&self, p: &Prop) -> f32 {
        f32::from_bits(self.prop_i32(p) as u32)
    }
    /// Object / name properties are stored as compact indices.
    pub fn prop_index(&self, p: &Prop) -> i32 {
        Reader::new(&self.data[..p.start + p.size], p.start).index().unwrap_or(0)
    }

    /// Lazy array header (version > 61: absolute skip offset before the count).
    pub fn lazy_count(&self, r: &mut Reader, item_size: usize) -> Result<usize> {
        if self.version > 61 {
            let _skip = r.i32()?;
        }
        r.count(item_size)
    }
}

pub fn find_prop<'a>(props: &'a [Prop], name: &str) -> Option<&'a Prop> {
    props.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}
