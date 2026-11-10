//! Readers for the original Deus Ex (2000, Unreal Engine 1) data files.
//!
//! Nothing from the original game is shipped with DEUSMOBILE: when a Deus Ex installation is
//! found on this machine its textures, sounds and character models are read at start-up
//! (see `dxassets.rs`); otherwise the procedural assets are used. Everything in this module
//! is plain Rust with no engine dependency, so it can be unit tested on its own.
pub mod character;
pub mod mesh;
pub mod package;
pub mod sound;
pub mod surface;
pub mod texture;

use std::path::{Path, PathBuf};

/// A file every Deus Ex install has; used to recognise the game folder.
const MARKER: (&str, &str) = ("Textures", "CoreTexMetal.utx");

fn is_install(p: &Path) -> bool {
    find_file(p, MARKER.0, MARKER.1).is_some()
}

/// Locate the game. Order: explicit path (`--dx PATH`), `DEUSEX_DIR`, a `deusex` folder next
/// to the executable or the working directory, then the usual Steam / GOG locations.
pub fn find_install(explicit: Option<&str>) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![];
    if let Some(p) = explicit {
        candidates.push(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("DEUSEX_DIR") {
        candidates.push(PathBuf::from(p));
    }
    candidates.push(PathBuf::from("deusex"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("deusex"));
        }
    }
    for drive in ["C:", "D:", "E:"] {
        for rel in [
            r"\Program Files (x86)\Steam\steamapps\common\Deus Ex",
            r"\Program Files\Steam\steamapps\common\Deus Ex",
            r"\SteamLibrary\steamapps\common\Deus Ex",
            r"\Steam\steamapps\common\Deus Ex",
            r"\GOG Games\Deus Ex GOTY",
            r"\GOG Games\Deus Ex",
            r"\Program Files (x86)\GOG Galaxy\Games\Deus Ex GOTY",
            r"\Program Files (x86)\GOG Galaxy\Games\Deus Ex",
            r"\DeusEx",
        ] {
            candidates.push(PathBuf::from(format!("{drive}{rel}")));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        for rel in [
            ".steam/steam/steamapps/common/Deus Ex",
            ".local/share/Steam/steamapps/common/Deus Ex",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/common/Deus Ex",
            "Library/Application Support/Steam/steamapps/common/Deus Ex",
        ] {
            candidates.push(Path::new(&home).join(rel));
        }
    }
    candidates.into_iter().find(|p| is_install(p))
}

/// Case-insensitive lookup of `root/dir/name` (the game's own folders are mixed case and
/// Linux file systems are not forgiving about it).
pub fn find_file(root: &Path, dir: &str, name: &str) -> Option<PathBuf> {
    let exact = root.join(dir).join(name);
    if exact.is_file() {
        return Some(exact);
    }
    let sub = std::fs::read_dir(root).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(dir))?.path();
    let file = std::fs::read_dir(&sub).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))?.path();
    file.is_file().then_some(file)
}

/// Open a package by folder and file name.
pub fn open(root: &Path, dir: &str, name: &str) -> Option<package::Package> {
    let path = find_file(root, dir, name)?;
    match package::Package::load(&path) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("deus ex: cannot read {}: {e}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::character::seq_frames;
    use super::mesh::AnimSeq;
    use super::package::Reader;
    use super::sound::decode_wav;
    use super::surface::mip_chain;

    #[test]
    fn compact_index() {
        // 0, 63, -5, 64 (two bytes), 8191 (two bytes), 8192 (three bytes)
        let data = [0x00, 0x3F, 0x85, 0x40, 0x01, 0x7F, 0x7F, 0x40, 0x80, 0x01];
        let mut r = Reader::new(&data, 0);
        assert_eq!(r.index().unwrap(), 0);
        assert_eq!(r.index().unwrap(), 63);
        assert_eq!(r.index().unwrap(), -5);
        assert_eq!(r.index().unwrap(), 64);
        assert_eq!(r.index().unwrap(), 8191);
        assert_eq!(r.index().unwrap(), 8192);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn wav_roundtrip() {
        let pcm = super::sound::Pcm { name: "t".into(), rate: 22050, samples: vec![0.0, 0.5, -0.5, 1.0], loop_range: None };
        let back = decode_wav("t", &pcm.to_wav(1.0)).unwrap();
        assert_eq!(back.rate, 22050);
        assert_eq!(back.samples.len(), 4);
        assert!((back.samples[1] - 0.5).abs() < 1e-3 && (back.samples[2] + 0.5).abs() < 1e-3);
    }

    #[test]
    fn mips_of_rectangles() {
        let levels = mip_chain(vec![255; 8 * 2 * 4], 8, 2, false);
        let sizes: Vec<usize> = levels.iter().map(|l| l.len() / 4).collect();
        assert_eq!(sizes, vec![16, 4, 2, 1]);
    }

    #[test]
    fn sequences() {
        let s = AnimSeq { name: "Walk".into(), group: "None".into(), start: 10, frames: 4, rate: 10.0 };
        assert_eq!(seq_frames(&s, 0.0, true), (10, 11, 0.0));
        let (a, b, _) = seq_frames(&s, 0.35, true);
        assert_eq!((a, b), (13, 10));
        assert_eq!(seq_frames(&s, 5.0, false), (13, 13, 0.0));
    }
}
