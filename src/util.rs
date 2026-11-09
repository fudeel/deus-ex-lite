//! Small math / random / noise helpers shared by every module.
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

/// Tiny xorshift RNG (deterministic, no external crate).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / 16_777_216.0
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f32()
    }
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
    pub fn unit_disc(&mut self) -> Vec2 {
        loop {
            let v = Vec2::new(self.range(-1.0, 1.0), self.range(-1.0, 1.0));
            if v.length_squared() <= 1.0 {
                return v;
            }
        }
    }
    pub fn unit_sphere(&mut self) -> Vec3 {
        loop {
            let v = Vec3::new(self.range(-1.0, 1.0), self.range(-1.0, 1.0), self.range(-1.0, 1.0));
            let l = v.length_squared();
            if l > 0.01 && l <= 1.0 {
                return v / l.sqrt();
            }
        }
    }
}

#[derive(Resource)]
pub struct GameRng(pub Rng);

impl Default for GameRng {
    fn default() -> Self {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        GameRng(Rng::new(t))
    }
}

pub fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393)
        ^ (y as u32).wrapping_mul(668_265_263)
        ^ seed.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    (h & 0x00ff_ffff) as f32 / 16_777_215.0
}

/// Tileable value noise. `period` is the number of lattice cells per tile.
pub fn value_noise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let xf = x - xi as f32;
    let yf = y - yi as f32;
    let u = xf * xf * (3.0 - 2.0 * xf);
    let v = yf * yf * (3.0 - 2.0 * yf);
    let p = |a: i32| a.rem_euclid(period.max(1));
    let a = hash2(p(xi), p(yi), seed);
    let b = hash2(p(xi + 1), p(yi), seed);
    let c = hash2(p(xi), p(yi + 1), seed);
    let d = hash2(p(xi + 1), p(yi + 1), seed);
    lerp(lerp(a, b, u), lerp(c, d, u), v)
}

/// Tileable fractal noise over texture coordinates in [0,1).
pub fn fbm(u: f32, v: f32, base_period: i32, octaves: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut norm = 0.0;
    let mut per = base_period;
    for o in 0..octaves {
        sum += amp * value_noise(u * per as f32, v * per as f32, per, seed.wrapping_add(o * 31));
        norm += amp;
        amp *= 0.5;
        per *= 2;
    }
    sum / norm
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn wrap_angle(a: f32) -> f32 {
    let mut a = a % TAU;
    if a > PI {
        a -= TAU;
    } else if a < -PI {
        a += TAU;
    }
    a
}

pub fn approach_angle(cur: f32, target: f32, max_step: f32) -> f32 {
    let d = wrap_angle(target - cur);
    wrap_angle(cur + d.clamp(-max_step, max_step))
}

pub fn v3(a: [f32; 3]) -> Vec3 {
    Vec3::from_array(a)
}

pub fn a3(v: Vec3) -> [f32; 3] {
    v.to_array()
}

/// Yaw 0 looks down -Z (Bevy forward).
pub fn yaw_dir(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}

pub fn dir_yaw(d: Vec3) -> f32 {
    (-d.x).atan2(-d.z)
}

pub fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
    let c = pitch.cos();
    Vec3::new(-yaw.sin() * c, pitch.sin(), -yaw.cos() * c)
}

/// Random direction inside a cone of half-angle `spread` (radians) around `dir`.
pub fn cone_dir(dir: Vec3, spread: f32, rng: &mut Rng) -> Vec3 {
    if spread <= 0.0 {
        return dir;
    }
    let d = rng.unit_disc() * spread.tan();
    let right = dir.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
    let up = right.cross(dir).normalize();
    (dir + right * d.x + up * d.y).normalize()
}

pub fn exp_decay(rate: f32, dt: f32) -> f32 {
    1.0 - (-rate * dt).exp()
}
