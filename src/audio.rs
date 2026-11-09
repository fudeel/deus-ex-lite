//! Procedurally synthesized sound effects (no audio files needed) + 3D playback.
use bevy::audio::{AudioSinkPlayback, PlaybackMode, Volume};
use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::TAU;

use crate::game::*;
use crate::util::Rng;

const SR: u32 = 22050;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum Sfx {
    Pistol,
    Rifle,
    Crossbow,
    Swing,
    Zap,
    GuardShot,
    TurretShot,
    ImpactWall,
    ImpactFlesh,
    ImpactMetal,
    Glass,
    Step,
    StepMetal,
    Land,
    DoorOpen,
    DoorClose,
    Locked,
    Unlock,
    Pickup,
    Click,
    Beep,
    Denied,
    Granted,
    Reload,
    Empty,
    Heal,
    AugOn,
    AugOff,
    Hurt,
    Chirp,
    Pick,
    Hack,
    Shatter,
    Radio,
    Alarm,
    Ambient,
    Rain,
    Spark,
}

impl Sfx {
    pub fn from_u8(v: u8) -> Option<Sfx> {
        ALL_SFX.get(v as usize).copied()
    }
}

const ALL_SFX: [Sfx; 38] = [
    Sfx::Pistol,
    Sfx::Rifle,
    Sfx::Crossbow,
    Sfx::Swing,
    Sfx::Zap,
    Sfx::GuardShot,
    Sfx::TurretShot,
    Sfx::ImpactWall,
    Sfx::ImpactFlesh,
    Sfx::ImpactMetal,
    Sfx::Glass,
    Sfx::Step,
    Sfx::StepMetal,
    Sfx::Land,
    Sfx::DoorOpen,
    Sfx::DoorClose,
    Sfx::Locked,
    Sfx::Unlock,
    Sfx::Pickup,
    Sfx::Click,
    Sfx::Beep,
    Sfx::Denied,
    Sfx::Granted,
    Sfx::Reload,
    Sfx::Empty,
    Sfx::Heal,
    Sfx::AugOn,
    Sfx::AugOff,
    Sfx::Hurt,
    Sfx::Chirp,
    Sfx::Pick,
    Sfx::Hack,
    Sfx::Shatter,
    Sfx::Radio,
    Sfx::Alarm,
    Sfx::Ambient,
    Sfx::Rain,
    Sfx::Spark,
];

#[derive(Event, Clone, Copy)]
pub struct SoundEv {
    pub sfx: Sfx,
    pub pos: Option<Vec3>,
    pub vol: f32,
}

impl SoundEv {
    pub fn at(sfx: Sfx, pos: Vec3, vol: f32) -> Self {
        SoundEv { sfx, pos: Some(pos), vol }
    }
    pub fn ui(sfx: Sfx, vol: f32) -> Self {
        SoundEv { sfx, pos: None, vol }
    }
}

#[derive(Resource)]
pub struct SoundBank(pub HashMap<Sfx, Handle<AudioSource>>);

#[derive(Component)]
pub struct RainLoop;

#[derive(Component)]
pub struct AlarmLoop;

#[derive(Component)]
pub struct AmbientLoop;

// ------------------------------------------------------------------ synthesis helpers

fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&SR.to_le_bytes());
    v.extend_from_slice(&(SR * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let i = (s.clamp(-1.0, 1.0) * 32000.0) as i16;
        v.extend_from_slice(&i.to_le_bytes());
    }
    v
}

struct Syn {
    rng: Rng,
}

impl Syn {
    fn n(&mut self) -> f32 {
        self.rng.f32() * 2.0 - 1.0
    }
    fn buf(secs: f32) -> Vec<f32> {
        vec![0.0; (secs * SR as f32) as usize]
    }
}

fn t_of(i: usize) -> f32 {
    i as f32 / SR as f32
}

fn lowpass(buf: &mut [f32], a: f32) {
    let mut y = 0.0;
    for s in buf.iter_mut() {
        y += a * (*s - y);
        *s = y;
    }
}

fn highpass(buf: &mut [f32], a: f32) {
    let mut lp = 0.0;
    for s in buf.iter_mut() {
        lp += a * (*s - lp);
        *s -= lp;
    }
}

fn normalize(buf: &mut [f32], peak: f32) {
    let m = buf.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if m > 1e-6 {
        for s in buf.iter_mut() {
            *s *= peak / m;
        }
    }
}

fn fade_out(buf: &mut [f32], secs: f32) {
    let n = ((secs * SR as f32) as usize).min(buf.len());
    let len = buf.len();
    for i in 0..n {
        buf[len - n + i] *= 1.0 - i as f32 / n as f32;
    }
}

fn gunshot(sy: &mut Syn, len: f32, thump: f32, decay: f32, tail: f32, bright: f32) -> Vec<f32> {
    let mut b = Syn::buf(len);
    let mut tailn = Syn::buf(len);
    for i in 0..b.len() {
        let t = t_of(i);
        let crack = sy.n() * (-t * decay).exp();
        let body = (TAU * thump * t * (1.0 - t * 0.8)).sin() * (-t * 18.0).exp() * 0.9;
        b[i] = crack * bright + body;
        tailn[i] = sy.n() * (-t * tail).exp() * 0.35;
    }
    lowpass(&mut tailn, 0.08);
    for i in 0..b.len() {
        b[i] += tailn[i];
    }
    lowpass(&mut b, 0.6);
    normalize(&mut b, 0.95);
    fade_out(&mut b, 0.05);
    b
}

fn tone(len: f32, f0: f32, f1: f32, decay: f32, square: bool) -> Vec<f32> {
    let mut b = Syn::buf(len);
    let mut ph = 0.0f32;
    for i in 0..b.len() {
        let t = t_of(i);
        let f = f0 + (f1 - f0) * (t / len);
        ph += TAU * f / SR as f32;
        let s = if square { ph.sin().signum() * 0.5 } else { ph.sin() };
        let att = (t * 400.0).min(1.0);
        b[i] = s * (-t * decay).exp() * att;
    }
    fade_out(&mut b, 0.01);
    b
}

fn concat(parts: &[Vec<f32>], gap: f32) -> Vec<f32> {
    let mut out = vec![];
    for (k, p) in parts.iter().enumerate() {
        if k > 0 {
            out.extend(std::iter::repeat_n(0.0, (gap * SR as f32) as usize));
        }
        out.extend_from_slice(p);
    }
    out
}

fn noise_burst(sy: &mut Syn, len: f32, decay: f32, lp: f32) -> Vec<f32> {
    let mut b = Syn::buf(len);
    for i in 0..b.len() {
        b[i] = sy.n() * (-t_of(i) * decay).exp();
    }
    lowpass(&mut b, lp);
    normalize(&mut b, 0.9);
    b
}

fn synth(sfx: Sfx, sy: &mut Syn) -> Vec<f32> {
    match sfx {
        Sfx::Pistol => gunshot(sy, 0.7, 95.0, 40.0, 5.0, 0.9),
        Sfx::Rifle => gunshot(sy, 0.45, 75.0, 55.0, 7.0, 1.0),
        Sfx::GuardShot => gunshot(sy, 0.6, 110.0, 45.0, 6.0, 0.8),
        Sfx::TurretShot => {
            let mut b = gunshot(sy, 0.25, 140.0, 70.0, 14.0, 0.8);
            let p = tone(0.25, 2200.0, 1800.0, 30.0, false);
            for i in 0..b.len() {
                b[i] += p[i] * 0.2;
            }
            b
        }
        Sfx::Crossbow => {
            let mut b = noise_burst(sy, 0.18, 40.0, 0.5);
            let t = tone(0.18, 520.0, 160.0, 22.0, false);
            for i in 0..b.len() {
                b[i] = b[i] * 0.4 + t[i] * 0.7;
            }
            b
        }
        Sfx::Swing => {
            let mut b = Syn::buf(0.3);
            for i in 0..b.len() {
                let t = t_of(i) / 0.3;
                b[i] = sy.n() * (t * std::f32::consts::PI).sin().powi(2);
            }
            lowpass(&mut b, 0.12);
            normalize(&mut b, 0.6);
            b
        }
        Sfx::Zap => {
            let mut b = Syn::buf(0.35);
            for i in 0..b.len() {
                let t = t_of(i);
                b[i] = ((TAU * 118.0 * t).sin().signum() * 0.5 + sy.n() * 0.5) * (-t * 9.0).exp();
            }
            lowpass(&mut b, 0.5);
            normalize(&mut b, 0.8);
            b
        }
        Sfx::ImpactWall => noise_burst(sy, 0.12, 50.0, 0.45),
        Sfx::ImpactFlesh => {
            let mut b = noise_burst(sy, 0.15, 35.0, 0.08);
            let t = tone(0.15, 120.0, 60.0, 25.0, false);
            for i in 0..b.len() {
                b[i] = b[i] * 0.6 + t[i] * 0.6;
            }
            b
        }
        Sfx::ImpactMetal => {
            let mut b = noise_burst(sy, 0.35, 60.0, 0.6);
            let t1 = tone(0.35, 1850.0, 1800.0, 9.0, false);
            let t2 = tone(0.35, 2730.0, 2700.0, 12.0, false);
            for i in 0..b.len() {
                b[i] = b[i] * 0.5 + t1[i] * 0.35 + t2[i] * 0.2;
            }
            b
        }
        Sfx::Glass | Sfx::Shatter => {
            let len = if sfx == Sfx::Shatter { 0.6 } else { 0.3 };
            let mut b = Syn::buf(len);
            for i in 0..b.len() {
                let t = t_of(i);
                let tink = if sy.rng.chance(0.004) { 1.0 } else { 0.0 };
                b[i] = sy.n() * (-t * 12.0).exp() * 0.5 + tink;
            }
            highpass(&mut b, 0.3);
            let mut tt = tone(len, 3100.0, 3000.0, 14.0, false);
            for i in 0..b.len() {
                tt[i] = b[i] + tt[i] * 0.2;
            }
            normalize(&mut tt, 0.8);
            tt
        }
        Sfx::Step => {
            let mut b = Syn::buf(0.09);
            for i in 0..b.len() {
                let t = t_of(i);
                b[i] = sy.n() * (t * 300.0).min(1.0) * (-t * 45.0).exp();
            }
            lowpass(&mut b, 0.15);
            normalize(&mut b, 0.5);
            b
        }
        Sfx::StepMetal => {
            let mut b = Syn::buf(0.18);
            for i in 0..b.len() {
                let t = t_of(i);
                b[i] = sy.n() * (-t * 50.0).exp() * 0.6 + (TAU * 410.0 * t).sin() * (-t * 22.0).exp() * 0.4;
            }
            lowpass(&mut b, 0.3);
            normalize(&mut b, 0.5);
            b
        }
        Sfx::Land => {
            let mut b = noise_burst(sy, 0.2, 25.0, 0.06);
            let t = tone(0.2, 90.0, 50.0, 20.0, false);
            for i in 0..b.len() {
                b[i] = b[i] * 0.5 + t[i] * 0.5;
            }
            b
        }
        Sfx::DoorOpen | Sfx::DoorClose => {
            let len = 0.7;
            let mut b = Syn::buf(len);
            let (f0, f1) = if sfx == Sfx::DoorOpen { (140.0, 260.0) } else { (260.0, 120.0) };
            let mut ph = 0.0f32;
            for i in 0..b.len() {
                let t = t_of(i);
                let f = f0 + (f1 - f0) * (t / len);
                ph += TAU * f / SR as f32;
                let env = (t * 8.0).min(1.0) * (1.0 - t / len);
                b[i] = (ph.sin() * 0.35 + (ph * 2.01).sin() * 0.15 + sy.n() * 0.25) * env;
            }
            lowpass(&mut b, 0.25);
            if sfx == Sfx::DoorClose {
                let thunk = noise_burst(sy, 0.15, 30.0, 0.05);
                let off = b.len() - thunk.len();
                for (i, s) in thunk.iter().enumerate() {
                    b[off + i] += s * 0.8;
                }
            }
            normalize(&mut b, 0.6);
            b
        }
        Sfx::Locked | Sfx::Denied => concat(&[tone(0.1, 220.0, 220.0, 2.0, true), tone(0.14, 180.0, 180.0, 2.0, true)], 0.05),
        Sfx::Unlock | Sfx::Granted => concat(&[tone(0.08, 880.0, 880.0, 4.0, false), tone(0.16, 1320.0, 1320.0, 6.0, false)], 0.02),
        Sfx::Pickup => tone(0.14, 700.0, 1400.0, 12.0, false),
        Sfx::Click => noise_burst(sy, 0.025, 200.0, 0.8),
        Sfx::Beep => tone(0.07, 1250.0, 1250.0, 10.0, false),
        Sfx::Reload => {
            let c1 = noise_burst(sy, 0.04, 120.0, 0.7);
            let c2 = noise_burst(sy, 0.05, 100.0, 0.5);
            concat(&[c1, c2], 0.25)
        }
        Sfx::Empty => noise_burst(sy, 0.03, 160.0, 0.9),
        Sfx::Heal => tone(0.5, 300.0, 700.0, 4.0, false),
        Sfx::AugOn => tone(0.35, 300.0, 1200.0, 5.0, false),
        Sfx::AugOff => tone(0.3, 900.0, 200.0, 6.0, false),
        Sfx::Hurt => {
            let mut b = noise_burst(sy, 0.25, 18.0, 0.05);
            let t = tone(0.25, 80.0, 50.0, 10.0, false);
            for i in 0..b.len() {
                b[i] = b[i] * 0.6 + t[i] * 0.6;
            }
            b
        }
        Sfx::Chirp => concat(&[tone(0.06, 1500.0, 1500.0, 8.0, true), tone(0.06, 2000.0, 2000.0, 8.0, true), tone(0.08, 1700.0, 1700.0, 8.0, true)], 0.02),
        Sfx::Pick => {
            let mut out = vec![];
            for _ in 0..5 {
                out.extend(noise_burst(sy, 0.02, 200.0, 0.9));
                out.extend(std::iter::repeat_n(0.0, (sy.rng.range(0.05, 0.12) * SR as f32) as usize));
            }
            out
        }
        Sfx::Hack => {
            let mut out = vec![];
            for _ in 0..8 {
                let f = sy.rng.range(900.0, 2600.0);
                out.extend(tone(0.04, f, f, 5.0, true));
                out.extend(std::iter::repeat_n(0.0, (0.02 * SR as f32) as usize));
            }
            out
        }
        Sfx::Radio => {
            let mut b = noise_burst(sy, 0.22, 4.0, 0.7);
            highpass(&mut b, 0.2);
            normalize(&mut b, 0.35);
            b
        }
        Sfx::Spark => {
            let mut b = Syn::buf(0.4);
            for i in 0..b.len() {
                let t = t_of(i);
                let pop = if sy.rng.chance(0.01) { sy.n() * 3.0 } else { 0.0 };
                b[i] = (sy.n() * 0.3 + pop) * (-t * 7.0).exp();
            }
            highpass(&mut b, 0.2);
            normalize(&mut b, 0.6);
            b
        }
        Sfx::Alarm => {
            let len = 2.0;
            let mut b = Syn::buf(len);
            let mut ph = 0.0f32;
            for i in 0..b.len() {
                let t = t_of(i);
                let f = 650.0 + 250.0 * (TAU * t / len).sin();
                ph += TAU * f / SR as f32;
                b[i] = (ph.sin() + 0.3 * (ph * 3.0).sin()) * 0.4;
            }
            b
        }
        Sfx::Ambient => {
            // seamless 8 second drone: every partial completes whole cycles
            let len = 8.0;
            let mut b = Syn::buf(len);
            let mut wind = Syn::buf(len);
            for i in 0..b.len() {
                let t = t_of(i);
                let lfo = 0.6 + 0.4 * (TAU * t / len).sin();
                let lfo2 = 0.5 + 0.5 * (TAU * 2.0 * t / len).cos();
                b[i] = (TAU * 55.0 * t).sin() * 0.30 * lfo
                    + (TAU * 82.5 * t).sin() * 0.18 * lfo2
                    + (TAU * 110.0 * t).sin() * 0.08
                    + (TAU * 164.875 * t).sin() * 0.04 * lfo;
                wind[i] = sy.n();
            }
            lowpass(&mut wind, 0.02);
            normalize(&mut wind, 1.0);
            for i in 0..b.len() {
                b[i] = b[i] * 0.7 + wind[i] * 0.25;
            }
            let fade = (0.5 * SR as f32) as usize;
            let n = b.len();
            for i in 0..fade {
                // crossfade the noise tail into the head so it loops cleanly
                let a = i as f32 / fade as f32;
                b[i] = b[i] * a + b[n - fade + i] * (1.0 - a);
            }
            b.truncate(n - fade);
            normalize(&mut b, 0.5);
            b
        }
        Sfx::Rain => {
            let len = 4.0;
            let mut b = Syn::buf(len);
            for i in 0..b.len() {
                let drop = if sy.rng.chance(0.002) { sy.n() * 2.0 } else { 0.0 };
                b[i] = sy.n() * 0.5 + drop;
            }
            lowpass(&mut b, 0.35);
            highpass(&mut b, 0.02);
            let fade = (0.4 * SR as f32) as usize;
            let n = b.len();
            for i in 0..fade {
                let a = i as f32 / fade as f32;
                b[i] = b[i] * a + b[n - fade + i] * (1.0 - a);
            }
            b.truncate(n - fade);
            normalize(&mut b, 0.5);
            b
        }
    }
}

pub fn setup_audio(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let t0 = std::time::Instant::now();
    let mut sy = Syn { rng: Rng::new(7) };
    let mut bank = HashMap::new();
    for s in ALL_SFX {
        let samples = synth(s, &mut sy);
        bank.insert(s, sources.add(AudioSource { bytes: wav(&samples).into() }));
    }
    info!("synthesized {} sounds in {:.2}s", bank.len(), t0.elapsed().as_secs_f32());
    commands.insert_resource(SoundBank(bank));
}

pub fn play_sounds(mut commands: Commands, mut evs: EventReader<SoundEv>, bank: Option<Res<SoundBank>>, settings: Res<Settings>) {
    let Some(bank) = bank else { return };
    for e in evs.read() {
        let Some(h) = bank.0.get(&e.sfx) else { continue };
        let vol = e.vol * settings.volume;
        let mut ps = PlaybackSettings { mode: PlaybackMode::Despawn, ..default() }.with_volume(Volume::Linear(vol));
        match e.pos {
            Some(p) => {
                ps = ps.with_spatial(true);
                commands.spawn((AudioPlayer::new(h.clone()), ps, Transform::from_translation(p)));
            }
            None => {
                commands.spawn((AudioPlayer::new(h.clone()), ps));
            }
        }
    }
}

/// Map relayed Fx::Sound events to local playback.
pub fn fx_sounds(mut fx: EventReader<FxEv>, mut out: EventWriter<SoundEv>) {
    for e in fx.read() {
        if let Fx::Sound { sfx, pos } = &e.fx {
            if let Some(s) = Sfx::from_u8(*sfx) {
                out.write(SoundEv::at(s, Vec3::from_array(*pos), 0.8));
            }
        }
    }
}

pub fn spawn_loops(commands: &mut Commands, bank: &SoundBank, settings: &Settings) {
    let v = settings.volume;
    if let Some(h) = bank.0.get(&Sfx::Ambient) {
        commands.spawn((
            AudioPlayer::new(h.clone()),
            PlaybackSettings { mode: PlaybackMode::Loop, ..default() }.with_volume(Volume::Linear(0.55 * v)),
            AmbientLoop,
            LevelEntity,
        ));
    }
    if let Some(h) = bank.0.get(&Sfx::Rain) {
        commands.spawn((
            AudioPlayer::new(h.clone()),
            PlaybackSettings { mode: PlaybackMode::Loop, ..default() }.with_volume(Volume::Linear(0.4 * v)),
            RainLoop,
            LevelEntity,
        ));
    }
}

/// Rain is loud outside and muffled indoors; alarm siren follows the world state.
pub fn update_loops(
    mut commands: Commands,
    mut rain: Query<&mut AudioSink, With<RainLoop>>,
    alarm_q: Query<Entity, With<AlarmLoop>>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    level: Option<Res<crate::level::Level>>,
    state: Res<WorldState>,
    bank: Option<Res<SoundBank>>,
    settings: Res<Settings>,
    time: Res<Time>,
    mut cur: Local<f32>,
) {
    let (Some(level), Some(bank)) = (level, bank) else { return };
    if let Ok(cam) = cam.single() {
        let indoor = level.indoor_at(cam.translation());
        let target = if indoor { 0.12 } else { 0.55 };
        *cur += (target - *cur) * (time.delta_secs() * 3.0).min(1.0);
        for mut s in rain.iter_mut() {
            s.set_volume(Volume::Linear(*cur * settings.volume));
        }
    }
    let playing = !alarm_q.is_empty();
    if state.alarm && !playing {
        if let Some(h) = bank.0.get(&Sfx::Alarm) {
            commands.spawn((
                AudioPlayer::new(h.clone()),
                PlaybackSettings { mode: PlaybackMode::Loop, ..default() }.with_volume(Volume::Linear(0.25 * settings.volume)),
                AlarmLoop,
                LevelEntity,
            ));
        }
    } else if !state.alarm && playing {
        for e in alarm_q.iter() {
            commands.entity(e).despawn();
        }
    }
}
