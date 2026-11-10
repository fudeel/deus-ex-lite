//! Original Deus Ex assets, read at start-up from the player's own copy of the game.
//!
//! DEUSMOBILE is open source and ships none of the original data. If Deus Ex (2000) is
//! installed (Steam / GOG paths are searched, or pass `--dx PATH` / set `DEUSEX_DIR`), this
//! plugin swaps the procedural placeholders for the real thing:
//!
//! * level surfaces, doors, grates and water use textures from `Textures/*.utx` (with a
//!   normal and roughness map derived from each one, so they still react to the lighting);
//! * sound effects come from `System/DeusExSounds.u`, `Sounds/MoverSFX.uax` and
//!   `Sounds/Ambient.uax`, level-matched to the synthesized mix;
//! * guards, the informant and team-match operatives become the original vertex-animated
//!   characters (NSF, UNATCO troopers, a smuggler) with their skins and animations, the
//!   held weapon riding on the model's own weapon triangle.
//!
//! Anything that cannot be found or read keeps its procedural version; `--no-dx` turns the
//! whole thing off.
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::primitives::Aabb as RenderAabb;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::{HashMap, HashSet};
use std::f32::consts::{FRAC_PI_2, PI};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::audio::{Sfx, SoundBank, SoundPeaks, SoundVariants};
use crate::avatars::Avatar;
use crate::dx;
use crate::dx::character::{build_character, seq_frames, Character};
use crate::dx::package::Package;
use crate::dx::sound::{find_sound, Pcm};
use crate::dx::surface::{build_pbr, mip_chain, PbrMaps, SurfaceLook};
use crate::dx::texture::{read_texture, TexImage};
use crate::game::*;
use crate::level::Surf;
use crate::materials::{repeat_sampler, Mats};
use crate::npc::{Civilian, Guard, Rig};

/// Deus Ex characters are ~95 units tall for ~1.8 m.
const UNITS_PER_M: f32 = 52.5;
/// cross-fade between animation sequences
const BLEND_TIME: f32 = 0.18;

#[derive(Resource, Clone, Default)]
pub struct DxConfig {
    /// `--dx PATH`
    pub path: Option<String>,
    /// `--no-dx`
    pub off: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Role {
    Guard,
    Informant,
    Orange,
    Blue,
}

pub struct DxModel {
    pub data: Arc<Character>,
    pub material: Handle<StandardMaterial>,
}

/// What was taken from the original game (empty when it is not installed).
#[derive(Resource, Default)]
pub struct DxAssets {
    pub root: Option<PathBuf>,
    pub textures: usize,
    pub sounds: usize,
    pub models: Vec<DxModel>,
    pub roles: HashMap<Role, usize>,
}

pub struct DxPlugin;

impl Plugin for DxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DxAssets>()
            .add_systems(Startup, load_dx_assets.after(crate::materials::setup_materials).after(crate::audio::setup_audio))
            .add_systems(Update, (attach_puppets, animate_puppets).chain().in_set(GameSet::Present));
    }
}

// ------------------------------------------------------------------ what to use

#[derive(Clone, Copy, Debug)]
enum Target {
    Surf(Surf),
    Containers,
    Door,
    DoorSec,
    Grate,
    Water,
}

struct SurfDef {
    target: Target,
    package: &'static str,
    texture: &'static str,
    /// multiplier on the level's UVs (the level maps 2 m per repeat on most surfaces; Deus
    /// Ex textures are drawn for ~2.4 m per 128 px, some at half that)
    uv: (f32, f32),
    look: SurfaceLook,
}

const fn look(bump: f32, rough: f32, rough_var: f32, metal: f32) -> SurfaceLook {
    SurfaceLook { bump, rough, rough_var, metal }
}

/// Server racks (blinking LEDs) and skirting trim keep their procedural look.
const SURFACES: &[SurfDef] = &[
    SurfDef { target: Target::Surf(Surf::Concrete), package: "CoreTexConcrete.utx", texture: "ClenGrayCemnt_A", uv: (0.8, 0.8), look: look(2.0, 0.85, 0.4, 0.0) },
    SurfDef { target: Target::Surf(Surf::Asphalt), package: "CoreTexConcrete.utx", texture: "RoadBlacktop__A", uv: (1.6, 1.6), look: look(1.5, 0.55, 0.6, 0.0) },
    SurfDef { target: Target::Surf(Surf::FloorTile), package: "CoreTexCeramic.utx", texture: "CermTileFloor_A", uv: (1.6, 1.6), look: look(3.0, 0.3, 0.3, 0.0) },
    SurfDef { target: Target::Surf(Surf::Carpet), package: "CoreTexTextile.utx", texture: "ClenBlueGrey_A", uv: (1.6, 1.6), look: look(1.5, 0.95, 0.0, 0.0) },
    SurfDef { target: Target::Surf(Surf::MetalFloor), package: "CoreTexMetal.utx", texture: "DiamPlatSteel_A", uv: (1.0, 1.0), look: look(4.0, 0.4, 0.4, 0.85) },
    SurfDef { target: Target::Surf(Surf::WallLab), package: "UNATCO.utx", texture: "UN_Wall_White", uv: (0.8, 0.8), look: look(2.5, 0.6, 0.3, 0.0) },
    SurfDef { target: Target::Surf(Surf::WallOffice), package: "CoreTexWood.utx", texture: "woodwall_a", uv: (1.6, 0.8), look: look(2.0, 0.5, 0.3, 0.0) },
    SurfDef { target: Target::Surf(Surf::WallMetal), package: "CoreTexMetal.utx", texture: "HangrMetlWall_D", uv: (1.0, 1.0), look: look(3.5, 0.5, 0.4, 0.7) },
    SurfDef { target: Target::Surf(Surf::WallExt), package: "CoreTexBrick.utx", texture: "DrtyReddBrick_A", uv: (1.64, 0.82), look: look(3.0, 0.85, 0.3, 0.0) },
    SurfDef { target: Target::Surf(Surf::WallBoundary), package: "Dockyard.utx", texture: "warewall01", uv: (0.82, 1.64), look: look(2.5, 0.8, 0.3, 0.2) },
    SurfDef { target: Target::Surf(Surf::Ceiling), package: "CoreTexTiles.utx", texture: "WhitCeilnTile_A", uv: (1.6, 1.6), look: look(2.0, 0.9, 0.2, 0.0) },
    SurfDef { target: Target::Surf(Surf::CeilingMetal), package: "CoreTexMetal.utx", texture: "ShipsCeiln_B", uv: (0.8, 0.8), look: look(3.0, 0.55, 0.3, 0.6) },
    SurfDef { target: Target::Surf(Surf::Roof), package: "CoreTexEarth.utx", texture: "ChipRockGravl_A", uv: (2.6, 2.6), look: look(3.0, 0.9, 0.2, 0.0) },
    SurfDef { target: Target::Surf(Surf::Vent), package: "CoreTexMetal.utx", texture: "Galvanzd_B", uv: (1.0, 1.0), look: look(1.5, 0.4, 0.3, 0.85) },
    SurfDef { target: Target::Surf(Surf::Crate), package: "CoreTexWood.utx", texture: "DrtyWoodCrate_A", uv: (2.0, 1.0), look: look(2.5, 0.75, 0.2, 0.0) },
    SurfDef { target: Target::Containers, package: "CoreTexMetal.utx", texture: "ShipYard_Box_A", uv: (1.54, 1.0), look: look(3.0, 0.55, 0.4, 0.4) },
    SurfDef { target: Target::Surf(Surf::Counter), package: "CoreTexStone.utx", texture: "ShnyBlacMarbl_A", uv: (1.0, 1.0), look: look(1.0, 0.2, 0.2, 0.0) },
    SurfDef { target: Target::Surf(Surf::WaterBed), package: "UNATCO.utx", texture: "MudLake_A", uv: (1.64, 1.64), look: look(2.0, 0.7, 0.2, 0.0) },
    SurfDef { target: Target::Door, package: "CoreTexMetal.utx", texture: "DrtyMetlDoor_A", uv: (1.0, 1.0), look: look(3.0, 0.45, 0.3, 0.7) },
    SurfDef { target: Target::DoorSec, package: "UNATCO.utx", texture: "un_BlastDoor_A", uv: (1.0, 1.0), look: look(3.5, 0.45, 0.3, 0.75) },
    SurfDef { target: Target::Grate, package: "CoreTexMetal.utx", texture: "metalgrate_a", uv: (1.0, 1.0), look: look(3.0, 0.5, 0.2, 0.8) },
    SurfDef { target: Target::Water, package: "CoreTexWater.utx", texture: "dirtywater", uv: (1.0, 1.0), look: look(0.0, 0.04, 0.0, 0.0) },
];

struct SoundDef {
    sfx: Sfx,
    dir: &'static str,
    package: &'static str,
    /// `Group.Name`; several entries = random takes
    names: &'static [&'static str],
    looping: bool,
}

const DXS: &str = "DeusExSounds.u";

/// Guard radio chatter, rain, the locked-door rattle and the unlock chime stay synthesized.
const SOUNDS: &[SoundDef] = &[
    SoundDef { sfx: Sfx::Pistol, dir: "System", package: DXS, names: &["Weapons.PistolFire"], looping: false },
    SoundDef { sfx: Sfx::Rifle, dir: "System", package: DXS, names: &["Weapons.AssaultGunFire"], looping: false },
    SoundDef { sfx: Sfx::Crossbow, dir: "System", package: DXS, names: &["Weapons.MiniCrossbowFire"], looping: false },
    SoundDef { sfx: Sfx::Swing, dir: "System", package: DXS, names: &["Weapons.BatonFire"], looping: false },
    SoundDef { sfx: Sfx::Zap, dir: "System", package: DXS, names: &["Weapons.ProdFire"], looping: false },
    SoundDef { sfx: Sfx::GuardShot, dir: "System", package: DXS, names: &["Weapons.AssaultGunFire"], looping: false },
    SoundDef { sfx: Sfx::TurretShot, dir: "System", package: DXS, names: &["Robot.RobotFireGun"], looping: false },
    SoundDef { sfx: Sfx::ImpactWall, dir: "System", package: DXS, names: &["Generic.RockHit1", "Generic.RockHit2", "Generic.Ricochet1", "Generic.Ricochet2"], looping: false },
    SoundDef { sfx: Sfx::ImpactFlesh, dir: "System", package: DXS, names: &["Generic.BulletHitFlesh", "Generic.FleshHit1", "Generic.FleshHit2"], looping: false },
    SoundDef { sfx: Sfx::ImpactMetal, dir: "System", package: DXS, names: &["Generic.MetalHit1", "Generic.MetalHit2"], looping: false },
    SoundDef { sfx: Sfx::Glass, dir: "System", package: DXS, names: &["Generic.GlassHit1", "Generic.GlassHit2"], looping: false },
    SoundDef { sfx: Sfx::Step, dir: "System", package: DXS, names: &["Player.StoneStep1", "Player.StoneStep2", "Player.StoneStep3", "Player.StoneStep4"], looping: false },
    SoundDef { sfx: Sfx::StepMetal, dir: "System", package: DXS, names: &["Player.MetalStep1", "Player.MetalStep2", "Player.MetalStep3", "Player.MetalStep4"], looping: false },
    SoundDef { sfx: Sfx::Land, dir: "System", package: DXS, names: &["Player.MaleLand"], looping: false },
    SoundDef { sfx: Sfx::DoorOpen, dir: "Sounds", package: "MoverSFX.uax", names: &["Door.SlideDoorOpen"], looping: false },
    SoundDef { sfx: Sfx::DoorClose, dir: "Sounds", package: "MoverSFX.uax", names: &["Door.SlideDoorClose"], looping: false },
    SoundDef { sfx: Sfx::Pickup, dir: "System", package: DXS, names: &["Generic.WeaponPickup"], looping: false },
    SoundDef { sfx: Sfx::Click, dir: "System", package: DXS, names: &["UserInterface.Menu_Press"], looping: false },
    SoundDef { sfx: Sfx::Beep, dir: "System", package: DXS, names: &["Generic.Beep1"], looping: false },
    SoundDef { sfx: Sfx::Denied, dir: "System", package: DXS, names: &["Generic.Buzz1"], looping: false },
    SoundDef { sfx: Sfx::Granted, dir: "System", package: DXS, names: &["Generic.Beep2"], looping: false },
    SoundDef { sfx: Sfx::Reload, dir: "System", package: DXS, names: &["Weapons.PistolReload"], looping: false },
    SoundDef { sfx: Sfx::Empty, dir: "System", package: DXS, names: &["Generic.DryFire"], looping: false },
    SoundDef { sfx: Sfx::Heal, dir: "System", package: DXS, names: &["Generic.MedicalHiss"], looping: false },
    SoundDef { sfx: Sfx::AugOn, dir: "System", package: DXS, names: &["Augmentation.AugActivate"], looping: false },
    SoundDef { sfx: Sfx::AugOff, dir: "System", package: DXS, names: &["Augmentation.AugDeactivate"], looping: false },
    SoundDef { sfx: Sfx::Hurt, dir: "System", package: DXS, names: &["Player.MalePainSmall", "Player.MalePainMedium"], looping: false },
    SoundDef { sfx: Sfx::Chirp, dir: "System", package: DXS, names: &["UserInterface.LogNoteAdded"], looping: false },
    SoundDef { sfx: Sfx::Pick, dir: "System", package: DXS, names: &["Generic.LockpickRattling"], looping: false },
    SoundDef { sfx: Sfx::Hack, dir: "System", package: DXS, names: &["Generic.KeyboardClick1", "Generic.KeyboardClick2", "Generic.KeyboardClick3"], looping: false },
    SoundDef { sfx: Sfx::Shatter, dir: "System", package: DXS, names: &["Generic.GlassBreakSmall", "Generic.GlassBreakLarge"], looping: false },
    SoundDef { sfx: Sfx::Spark, dir: "System", package: DXS, names: &["Generic.Spark1", "Generic.Spark2"], looping: false },
    SoundDef { sfx: Sfx::Alarm, dir: "Sounds", package: "Ambient.uax", names: &["Ambient.Klaxon2"], looping: true },
    SoundDef { sfx: Sfx::Ambient, dir: "Sounds", package: "Ambient.uax", names: &["Ambient.HarborWind"], looping: true },
];

struct CharDef {
    role: Role,
    mesh_package: &'static str,
    mesh: &'static str,
    /// (material texture slot, skin in DeusExCharacters.u); other slots are not drawn
    skins: &'static [(usize, &'static str)],
}

const NSF: &[(usize, &str)] = &[(1, "TerroristTex2"), (2, "TerroristTex1"), (3, "TerroristTex0"), (4, "TerroristTex0")];
const NSF_CAP: &[(usize, &str)] = &[(1, "TerroristTex2"), (2, "TerroristTex1"), (3, "TerroristTex0"), (4, "TerroristTex0"), (6, "TerroristTex0")];
const UNATCO: &[(usize, &str)] = &[(1, "UNATCOTroopTex1"), (2, "UNATCOTroopTex2"), (3, "MiscTex1"), (4, "MiscTex1"), (6, "UNATCOTroopTex3")];
const SMUGGLER: &[(usize, &str)] = &[(0, "SmugglerTex0"), (1, "SmugglerTex1"), (2, "SmugglerTex2"), (4, "TrenchShirtTex1"), (5, "SmugglerTex1")];

/// Campaign guards are NSF terrorists; in the team match Orange plays NSF and Blue UNATCO
/// on the multiplayer model (it has the crouch-walk the single-player one lacks).
const CHARACTERS: &[CharDef] = &[
    CharDef { role: Role::Guard, mesh_package: "DeusExCharacters.u", mesh: "GM_Jumpsuit", skins: NSF_CAP },
    CharDef { role: Role::Informant, mesh_package: "DeusExCharacters.u", mesh: "GM_Trench", skins: SMUGGLER },
    CharDef { role: Role::Orange, mesh_package: "MPCharacters.u", mesh: "mp_jumpsuit", skins: NSF },
    CharDef { role: Role::Blue, mesh_package: "MPCharacters.u", mesh: "mp_jumpsuit", skins: UNATCO },
];

// ------------------------------------------------------------------ decoding (worker threads)

struct Packages<'a> {
    root: &'a Path,
    open: HashMap<String, Option<Package>>,
}

impl<'a> Packages<'a> {
    fn new(root: &'a Path) -> Self {
        Packages { root, open: HashMap::new() }
    }
    fn get(&mut self, dir: &str, file: &str) -> Option<&Package> {
        let key = format!("{dir}/{file}");
        let root = self.root;
        self.open.entry(key).or_insert_with(|| dx::open(root, dir, file)).as_ref()
    }
}

fn decode_surfaces(root: &Path) -> Vec<(Target, PbrMaps, (f32, f32))> {
    let mut pk = Packages::new(root);
    let mut out = vec![];
    for d in SURFACES {
        let Some(pkg) = pk.get("Textures", d.package) else { continue };
        let Some(i) = pkg.find(d.texture, "Texture") else {
            warn!("deus ex: texture {} not found in {}", d.texture, d.package);
            continue;
        };
        match read_texture(pkg, i) {
            Ok(t) => out.push((d.target, build_pbr(&t, d.look), d.uv)),
            Err(e) => warn!("deus ex: {e}"),
        }
    }
    out
}

fn decode_sounds(root: &Path) -> Vec<(Sfx, Vec<Pcm>, bool)> {
    let mut pk = Packages::new(root);
    let mut out = vec![];
    for d in SOUNDS {
        let Some(pkg) = pk.get(d.dir, d.package) else { continue };
        let takes: Vec<Pcm> = d
            .names
            .iter()
            .filter_map(|n| match find_sound(pkg, n) {
                Ok(p) if !p.samples.is_empty() => Some(p),
                Ok(_) => None,
                Err(e) => {
                    warn!("deus ex: {e}");
                    None
                }
            })
            .collect();
        if !takes.is_empty() {
            out.push((d.sfx, takes, d.looping));
        }
    }
    out
}

fn decode_characters(root: &Path) -> Vec<(Role, Character)> {
    let mut pk = Packages::new(root);
    let mut out = vec![];
    let mut skins: HashMap<&str, Option<TexImage>> = HashMap::new();
    for d in CHARACTERS {
        for (_, name) in d.skins {
            if !skins.contains_key(name) {
                let tex = pk.get("System", "DeusExCharacters.u").and_then(|p| p.find(name, "Texture").and_then(|i| read_texture(p, i).map_err(|e| warn!("deus ex: {e}")).ok()));
                skins.insert(*name, tex);
            }
        }
        let Some(pkg) = pk.get("System", d.mesh_package) else { continue };
        let Some(i) = pkg.find(d.mesh, "LodMesh") else {
            warn!("deus ex: mesh {} not found", d.mesh);
            continue;
        };
        let mesh = match dx::mesh::read_lodmesh(pkg, i) {
            Ok(m) => m,
            Err(e) => {
                warn!("deus ex: {e}");
                continue;
            }
        };
        if d.skins.iter().any(|(_, n)| skins.get(n).map(|t| t.is_none()).unwrap_or(true)) {
            warn!("deus ex: skins for {} are missing, keeping the procedural model", d.mesh);
            continue;
        }
        let mut slots: Vec<Option<&TexImage>> = vec![None; 8];
        for (slot, name) in d.skins {
            if *slot < slots.len() {
                slots[*slot] = skins.get(name).and_then(|t| t.as_ref());
            }
        }
        out.push((d.role, build_character(&mesh, &slots, UNITS_PER_M)));
    }
    out
}

// ------------------------------------------------------------------ start-up

fn make_image(w: u32, h: u32, levels: Vec<Vec<u8>>, srgb: bool) -> Image {
    let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
    let mut img = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, levels[0].clone(), format, RenderAssetUsages::RENDER_WORLD);
    img.texture_descriptor.mip_level_count = levels.len() as u32;
    img.data = Some(levels.concat());
    img.sampler = ImageSampler::Descriptor(repeat_sampler());
    img
}

fn material_handles(mats: &Mats, t: Target) -> Vec<Handle<StandardMaterial>> {
    match t {
        Target::Surf(s) => mats.surf.get(&s).cloned().into_iter().collect(),
        Target::Containers => (0..5u8).filter_map(|i| mats.surf.get(&Surf::Container(i)).cloned()).collect(),
        Target::Door => vec![mats.door.clone()],
        Target::DoorSec => vec![mats.door_sec.clone()],
        Target::Grate => vec![mats.grate.clone()],
        Target::Water => vec![mats.water.clone()],
    }
}

#[allow(clippy::too_many_arguments)]
pub fn load_dx_assets(
    cfg: Option<Res<DxConfig>>,
    mats: Option<Res<Mats>>,
    mut dxa: ResMut<DxAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut bank: Option<ResMut<SoundBank>>,
    peaks: Option<Res<SoundPeaks>>,
    mut variants: Option<ResMut<SoundVariants>>,
) {
    let cfg = cfg.map(|c| (*c).clone()).unwrap_or_default();
    if cfg.off {
        info!("deus ex: original assets disabled (--no-dx)");
        return;
    }
    let Some(root) = dx::find_install(cfg.path.as_deref()) else {
        info!("deus ex: no installation found (use --dx PATH or DEUSEX_DIR); using procedural assets");
        return;
    };
    info!("deus ex: using original assets from {}", root.display());
    let t0 = std::time::Instant::now();
    let (surfaces, sounds, characters) = std::thread::scope(|s| {
        let a = s.spawn(|| decode_surfaces(&root));
        let b = s.spawn(|| decode_sounds(&root));
        let c = s.spawn(|| decode_characters(&root));
        (a.join().unwrap_or_default(), b.join().unwrap_or_default(), c.join().unwrap_or_default())
    });

    // ---- surfaces: swap the textures inside the existing materials, so every handle that
    // was already handed out picks them up
    if let Some(mats) = mats.as_deref() {
        for (target, maps, uv) in surfaces {
            let (w, h) = (maps.width, maps.height);
            let albedo = images.add(make_image(w, h, maps.albedo, true));
            if matches!(target, Target::Water) {
                // keep the procedural ripple normals; the murky colour comes from Deus Ex
                if let Some(m) = materials.get_mut(&mats.water) {
                    m.base_color_texture = Some(albedo);
                    m.base_color = Color::srgb(0.32, 0.34, 0.36);
                }
                dxa.textures += 1;
                continue;
            }
            let normal = images.add(make_image(w, h, maps.normal, false));
            let mr = images.add(make_image(w, h, maps.mr, false));
            for handle in material_handles(mats, target) {
                if let Some(m) = materials.get_mut(&handle) {
                    m.base_color_texture = Some(albedo.clone());
                    m.normal_map_texture = Some(normal.clone());
                    m.metallic_roughness_texture = Some(mr.clone());
                    m.metallic = 1.0;
                    m.perceptual_roughness = 1.0;
                    m.emissive = LinearRgba::BLACK;
                    m.emissive_texture = None;
                    m.uv_transform = Affine2::from_scale(Vec2::new(uv.0, uv.1));
                }
            }
            dxa.textures += 1;
        }
    }

    // ---- sounds
    if let Some(bank) = bank.as_deref_mut() {
        for (sfx, takes, looping) in sounds {
            let target_peak = peaks.as_ref().and_then(|p| p.0.get(&sfx).copied()).filter(|p| *p > 0.01).unwrap_or(0.7);
            let src_peak = takes.iter().map(|t| t.peak()).fold(0.0f32, f32::max).max(0.01);
            let gain = (target_peak / src_peak).min(4.0);
            let handles: Vec<Handle<AudioSource>> = takes
                .into_iter()
                .map(|mut pcm| {
                    if looping {
                        // loop exactly the region the sound designer marked
                        if let Some((a, b)) = pcm.loop_range {
                            let (a, b) = (a as usize, (b as usize).min(pcm.samples.len()));
                            if b > a + 64 {
                                pcm.samples = pcm.samples[a..b].to_vec();
                            }
                        }
                    }
                    sources.add(AudioSource { bytes: pcm.to_wav(gain).into() })
                })
                .collect();
            bank.0.insert(sfx, handles[0].clone());
            if let Some(v) = variants.as_deref_mut() {
                if handles.len() > 1 {
                    v.0.insert(sfx, handles);
                } else {
                    v.0.remove(&sfx);
                }
            }
            dxa.sounds += 1;
        }
    }

    // ---- characters
    for (role, c) in characters {
        let img = images.add(make_image(c.atlas_size, c.atlas_size, mip_chain(c.atlas.clone(), c.atlas_size, c.atlas_size, false), true));
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(img),
            alpha_mode: AlphaMode::Mask(0.5),
            double_sided: true,
            cull_mode: None,
            perceptual_roughness: 0.72,
            reflectance: 0.3,
            ..default()
        });
        dxa.models.push(DxModel { data: Arc::new(c), material });
        let idx = dxa.models.len() - 1;
        dxa.roles.insert(role, idx);
    }
    info!(
        "deus ex: {} textures, {} sounds, {} characters loaded in {:.2}s",
        dxa.textures,
        dxa.sounds,
        dxa.models.len(),
        t0.elapsed().as_secs_f32()
    );
    dxa.root = Some(root);
}

// ------------------------------------------------------------------ animated characters

#[derive(Component)]
pub struct DxPuppet {
    model: usize,
    mesh: Handle<Mesh>,
    socket: Entity,
    seq: &'static str,
    looping: bool,
    time: f32,
    blend: f32,
    from: Vec<[f32; 3]>,
    from_n: Vec<[f32; 3]>,
    pose: Vec<[f32; 3]>,
    pose_n: Vec<[f32; 3]>,
    key: (usize, usize, u16, bool),
}

fn character_mesh(c: &Character, pose: &[[f32; 3]], pose_n: &[[f32; 3]]) -> Mesh {
    let pos: Vec<[f32; 3]> = c.src.iter().map(|&s| pose[s as usize]).collect();
    let nrm: Vec<[f32; 3]> = c.src.iter().map(|&s| pose_n[s as usize]).collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, c.uv.clone())
        .with_inserted_indices(Indices::U32(c.indices.clone()))
}

/// Give every new guard / informant / operative its Deus Ex model. The procedural rig stays
/// (its joints are still animated by the AI and network code) but is no longer drawn; the
/// held weapon moves onto the model's weapon triangle.
#[allow(clippy::type_complexity)]
fn attach_puppets(
    mut commands: Commands,
    dxa: Res<DxAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    new: Query<(Entity, &Rig, Option<&Guard>, Option<&Avatar>, Has<Civilian>), Added<Rig>>,
    children: Query<&Children>,
    drawn: Query<(), With<Mesh3d>>,
) {
    if dxa.models.is_empty() {
        return;
    }
    for (root, rig, guard, avatar, civilian) in new.iter() {
        let role = if guard.is_some() {
            Role::Guard
        } else if let Some(a) = avatar {
            if a.team == TEAM_BLUE {
                Role::Blue
            } else {
                Role::Orange
            }
        } else if civilian {
            Role::Informant
        } else {
            continue;
        };
        let Some(&mi) = dxa.roles.get(&role) else { continue };
        let model = &dxa.models[mi];
        let c = &model.data;

        let keep: HashSet<Entity> = match rig.hand {
            Some(h) => std::iter::once(h).chain(children.iter_descendants(h)).collect(),
            None => HashSet::new(),
        };
        for e in children.iter_descendants(rig.body) {
            if drawn.contains(e) && !keep.contains(&e) {
                commands.entity(e).insert(Visibility::Hidden);
            }
        }

        let seq = "BreatheLight";
        let first = c.seq(seq).map(|s| s.start as usize).unwrap_or(0);
        let (mut pose, mut pose_n) = (vec![], vec![]);
        c.pose(first, first, 0.0, &mut pose, &mut pose_n);
        let mesh = meshes.add(character_mesh(c, &pose, &pose_n));
        let body = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(model.material.clone()),
                Transform::from_xyz(0.0, c.foot, 0.0),
                // generous fixed bounds: the pose changes every frame (lying down, reaching)
                RenderAabb::from_min_max(Vec3::new(-1.4, -1.3, -1.4), Vec3::new(1.4, 1.3, 1.4)),
                Visibility::default(),
            ))
            .id();
        let socket = commands.spawn((Transform::default(), Visibility::default())).id();
        commands.entity(root).add_child(body).add_child(socket);
        if let Some(h) = rig.hand {
            // same orientation the mount had on the procedural forearm (see `spawn_humanoid`)
            commands.entity(socket).add_child(h);
            commands.entity(h).insert(Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2) * Quat::from_rotation_z(PI)));
        }
        commands.entity(root).insert(DxPuppet {
            model: mi,
            mesh,
            socket,
            seq,
            looping: true,
            time: 0.0,
            blend: 0.0,
            from: vec![],
            from_n: vec![],
            pose,
            pose_n,
            key: (usize::MAX, 0, 0, false),
        });
    }
}

/// (sequence, loops, playback speed)
fn guard_anim(g: &Guard) -> (&'static str, bool, f32) {
    let r = g.rifle;
    match g.anim() {
        5 | 6 => ("DeathFront", false, 1.0),
        7 => ("HitTorso", true, 0.5),
        3 => (if r { "Shoot2H" } else { "Shoot" }, true, 0.0),
        2 => (if r { "RunShoot2H" } else { "Run" }, true, (g.speed / 4.0).clamp(0.6, 1.5)),
        1 => (if r { "Walk2H" } else { "Walk" }, true, (g.speed / 1.6).clamp(0.5, 1.6)),
        _ => (if r { "BreatheLight2H" } else { "BreatheLight" }, true, 1.0),
    }
}

fn avatar_anim(a: &Avatar, players: &Players) -> (&'static str, bool, f32) {
    let Some(p) = players.map.get(&a.id) else { return ("BreatheLight", true, 1.0) };
    if p.net.has(pflags::DOWNED) {
        return ("DeathFront", false, 1.0);
    }
    let two = matches!(p.net.weapon, 2 | 3);
    let gun = matches!(p.net.weapon, 1..=3);
    if a.crouch > 0.5 {
        return if a.speed > 0.4 { ("CrouchWalk", true, (a.speed / 1.2).clamp(0.6, 1.6)) } else { ("Crouch", false, 1.0) };
    }
    if a.speed > 3.0 {
        let s = if two {
            "RunShoot2H"
        } else if gun {
            "RunShoot"
        } else {
            "Run"
        };
        return (s, true, (a.speed / 5.0).clamp(0.7, 1.6));
    }
    if a.speed > 0.4 {
        return (if two { "Walk2H" } else { "Walk" }, true, (a.speed / 2.0).clamp(0.5, 1.8));
    }
    (if two { "BreatheLight2H" } else { "BreatheLight" }, true, 1.0)
}

fn animate_puppets(
    time: Res<Time>,
    dxa: Res<DxAssets>,
    players: Res<Players>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut q: Query<(&mut DxPuppet, Option<&Guard>, Option<&Avatar>)>,
    mut sockets: Query<&mut Transform, Without<DxPuppet>>,
) {
    let dt = time.delta_secs().min(0.1);
    for (mut pup, guard, avatar) in q.iter_mut() {
        let Some(model) = dxa.models.get(pup.model) else { continue };
        let c = &model.data;
        let (name, looping, rate) = match (guard, avatar) {
            (Some(g), _) => guard_anim(g),
            (None, Some(a)) => avatar_anim(a, &players),
            _ => ("BreatheLight", true, 1.0),
        };
        let p = &mut *pup;
        if p.seq != name {
            p.from = p.pose.clone();
            p.from_n = p.pose_n.clone();
            p.blend = if p.from.is_empty() { 0.0 } else { 1.0 };
            p.seq = name;
            p.looping = looping;
            p.time = 0.0;
        }
        p.time += dt * rate;
        p.blend = (p.blend - dt / BLEND_TIME).max(0.0);
        let Some(s) = c.seq(p.seq).or_else(|| c.seq("Still")) else { continue };
        let (a, b, t) = seq_frames(s, p.time, p.looping);
        let key = (a, b, (t * 64.0) as u16, p.blend > 0.0);
        if key == p.key {
            continue;
        }
        p.key = key;
        c.pose(a, b, t, &mut p.pose, &mut p.pose_n);
        if p.blend > 0.0 && p.from.len() == p.pose.len() {
            let w = p.blend * p.blend * (3.0 - 2.0 * p.blend);
            for i in 0..p.pose.len() {
                for k in 0..3 {
                    p.pose[i][k] += (p.from[i][k] - p.pose[i][k]) * w;
                    p.pose_n[i][k] += (p.from_n[i][k] - p.pose_n[i][k]) * w;
                }
            }
        }
        if let Some(m) = meshes.get_mut(&p.mesh) {
            let pos: Vec<[f32; 3]> = c.src.iter().map(|&s| p.pose[s as usize]).collect();
            let nrm: Vec<[f32; 3]> = c.src.iter().map(|&s| p.pose_n[s as usize]).collect();
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
        }
        if let Some((o, f, d)) = c.weapon_frame(&p.pose) {
            if let Ok(mut tf) = sockets.get_mut(p.socket) {
                let (f, d) = (Vec3::from_array(f), Vec3::from_array(d));
                let y = -f;
                let x = y.cross(d);
                tf.translation = Vec3::from_array(o) + Vec3::Y * c.foot;
                tf.rotation = Quat::from_mat3(&Mat3::from_cols(x, y, d));
            }
        }
    }
}
