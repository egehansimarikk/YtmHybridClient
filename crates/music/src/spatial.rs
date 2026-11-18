//! Cross-Platform 3D Spatial Audio & Virtual Surround Engine.
//!
//! Runs identically across Linux (PipeWire/PulseAudio/ALSA), Windows (WASAPI),
//! and macOS (CoreAudio) inside the universal `rodio::Source` DSP pipeline.
//!
//! Architecture:
//! 1. Complementary `libbs2b` Bauer crossfeed (`H_direct + H_cross == 1.0`)
//! 2. Virtual 5.1/7.1 speaker array simulation (Front L/R, Wide Side L/R, Rear Surround L/R)
//!    using true Interaural Time Difference (ITD) & head-shadow low-pass filtering
//! 3. Cascaded 2nd-order Biquad All-Pass phase decorrelators for out-of-head 3D projection
//! 4. Pinna HRTF elevation & front-stage spectral cueing
//! 5. Stereo Schroeder acoustic room & concert hall engine with sub-bass mono lock

use std::f32::consts::{PI, TAU};
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rodio::source::SeekError;
use rodio::Source;

#[allow(dead_code)]
pub const AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM: u32 = 0x8000_0000;
#[allow(dead_code)]
pub const AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY: u32 = 0x0800_0000;

const MAX_DELAY_CAPACITY: usize = 16384;
const DEFAULT_RAMP_DURATION: Duration = Duration::from_millis(30);

// ==============================================================================
// Error Handling
// ==============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum AudioError {
    DeviceUnavailable(String),
    UnsupportedFormat { channels: u16, sample_rate: u32 },
    BufferUnderflow,
    ChannelMismatch { expected: usize, found: usize },
    LockError(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeviceUnavailable(msg) => write!(f, "Audio output device unavailable: {msg}"),
            Self::UnsupportedFormat {
                channels,
                sample_rate,
            } => write!(f, "Unsupported audio format: {channels}ch @ {sample_rate} Hz"),
            Self::BufferUnderflow => write!(f, "Audio stream buffer underflow"),
            Self::ChannelMismatch { expected, found } => {
                write!(f, "Audio channel count mismatch: expected {expected}, found {found}")
            }
            Self::LockError(msg) => write!(f, "Audio processor lock error: {msg}"),
        }
    }
}

impl std::error::Error for AudioError {}

// ==============================================================================
// Spatial Audio Modes & Presets
// ==============================================================================

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[repr(u32)]
pub enum SpatialMode {
    #[default]
    Off = 0,
    Subtle = 1,
    Studio = 2,
    FrontStage = 3,
    WideStage = 4,
    ConcertHall = 5,
}

impl SpatialMode {
    pub const ALL: [Self; 6] = [
        Self::Off,
        Self::Subtle,
        Self::Studio,
        Self::FrontStage,
        Self::WideStage,
        Self::ConcertHall,
    ];

    #[inline]
    pub const fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Subtle,
            2 => Self::Studio,
            3 => Self::FrontStage,
            4 => Self::WideStage,
            5 => Self::ConcertHall,
            _ => Self::Off,
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Subtle => "subtle",
            Self::Studio => "studio",
            Self::FrontStage => "front_stage",
            Self::WideStage => "wide_stage",
            Self::ConcertHall => "concert_hall",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Subtle => "Subtle (Meier)",
            Self::Studio => "Studio (Monitors)",
            Self::FrontStage => "Front Stage (HRTF)",
            Self::WideStage => "3D Surround (Wide)",
            Self::ConcertHall => "3D Concert Hall",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Off => "Bit-exact passthrough with zero DSP processing",
            Self::Subtle => "Jan Meier natural crossfeed for fatigue-free IEM listening",
            Self::Studio => "Virtual near-field studio monitors with tight room acoustics",
            Self::FrontStage => "Out-of-head Pinna HRTF + ITD projection placing vocals in front of you",
            Self::WideStage => "Virtual 7.1 surround expansion with phase decorrelation & deep stage",
            Self::ConcertHall => "Full 3D acoustic concert hall with multi-tap Schroeder reverberation",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "off" => Some(Self::Off),
            "subtle" => Some(Self::Subtle),
            "studio" => Some(Self::Studio),
            "front_stage" => Some(Self::FrontStage),
            "wide_stage" => Some(Self::WideStage),
            "concert_hall" => Some(Self::ConcertHall),
            _ => None,
        }
    }

    pub const fn params(self) -> SpatialParams {
        match self {
            Self::Off => SpatialParams {
                width: 0.0,
                depth: 0.0,
                crossfeed_gain: 0.0,
                hrtf_gain: 0.0,
                reflection_gain: 0.0,
            },
            Self::Subtle => SpatialParams {
                width: 0.25,
                depth: 0.22,
                crossfeed_gain: 0.35,
                hrtf_gain: 0.20,
                reflection_gain: 0.04,
            },
            Self::Studio => SpatialParams {
                width: 0.55,
                depth: 0.48,
                crossfeed_gain: 0.60,
                hrtf_gain: 0.45,
                reflection_gain: 0.16,
            },
            Self::FrontStage => SpatialParams {
                width: 0.78,
                depth: 0.90,
                crossfeed_gain: 0.52,
                hrtf_gain: 1.0,
                reflection_gain: 0.25,
            },
            Self::WideStage => SpatialParams {
                width: 1.0,
                depth: 0.85,
                crossfeed_gain: 0.32,
                hrtf_gain: 0.78,
                reflection_gain: 0.35,
            },
            Self::ConcertHall => SpatialParams {
                width: 1.0,
                depth: 1.0,
                crossfeed_gain: 0.35,
                hrtf_gain: 0.88,
                reflection_gain: 0.72,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpatialParams {
    pub width: f32,
    pub depth: f32,
    pub crossfeed_gain: f32,
    pub hrtf_gain: f32,
    pub reflection_gain: f32,
}

impl Default for SpatialParams {
    fn default() -> Self {
        SpatialMode::Off.params()
    }
}

// ==============================================================================
// Complementary First-Order Bauer Crossfeed (libbs2b Exact Topology)
// ==============================================================================

#[derive(Clone, Copy, Debug)]
struct Bs2bFilterPair {
    dir_b0: f32,
    dir_b1: f32,
    cross_b0: f32,
    cross_b1: f32,
    a1: f32,
    dir_x1: f32,
    dir_y1: f32,
    cross_x1: f32,
    cross_y1: f32,
}

impl Bs2bFilterPair {
    fn new(cutoff_hz: f32, feed_db: f32, sample_rate: f32) -> Self {
        let g_level = 10.0f32.powf(-feed_db.clamp(1.0, 15.0) / 20.0);
        let d = 1.0 / (1.0 + g_level);
        let c = 1.0 - d;

        let fc = cutoff_hz.clamp(200.0, sample_rate * 0.25);
        let k = (PI * fc / sample_rate).tan();
        let denom = 1.0 + k;

        Self {
            dir_b0: (1.0 + d * k) / denom,
            dir_b1: (d * k - 1.0) / denom,
            cross_b0: (c * k) / denom,
            cross_b1: (c * k) / denom,
            a1: (k - 1.0) / denom,
            dir_x1: 0.0,
            dir_y1: 0.0,
            cross_x1: 0.0,
            cross_y1: 0.0,
        }
    }

    #[inline(always)]
    fn process_direct(&mut self, x: f32) -> f32 {
        let y = self.dir_b0 * x + self.dir_b1 * self.dir_x1 - self.a1 * self.dir_y1;
        self.dir_x1 = x;
        self.dir_y1 = y;
        y
    }

    #[inline(always)]
    fn process_cross(&mut self, x: f32) -> f32 {
        let y = self.cross_b0 * x + self.cross_b1 * self.cross_x1 - self.a1 * self.cross_y1;
        self.cross_x1 = x;
        self.cross_y1 = y;
        y
    }

    fn reset(&mut self) {
        self.dir_x1 = 0.0;
        self.dir_y1 = 0.0;
        self.cross_x1 = 0.0;
        self.cross_y1 = 0.0;
    }
}

// ==============================================================================
// 2nd-Order Biquad Filters (HRTF Peaking & 360-Degree All-Pass Phase Rotators)
// ==============================================================================

#[derive(Clone, Copy, Debug)]
struct BiquadStereo {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1_l: f32,
    z2_l: f32,
    z1_r: f32,
    z2_r: f32,
}

impl BiquadStereo {
    fn peaking(freq_hz: f32, gain_db: f32, q: f32, sample_rate: f32) -> Self {
        let fc = freq_hz.clamp(120.0, sample_rate * 0.45);
        let a = 10.0f32.powf(gain_db / 40.0);
        let w0 = TAU * fc / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q.max(0.2));
        let a0 = 1.0 + alpha / a;

        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha / a) / a0,
            z1_l: 0.0,
            z2_l: 0.0,
            z1_r: 0.0,
            z2_r: 0.0,
        }
    }

    #[inline(always)]
    fn process_stereo(&mut self, l: f32, r: f32) -> (f32, f32) {
        let yl = self.b0 * l + self.z1_l;
        self.z1_l = self.b1 * l - self.a1 * yl + self.z2_l;
        self.z2_l = self.b2 * l - self.a2 * yl;

        let yr = self.b0 * r + self.z1_r;
        self.z1_r = self.b1 * r - self.a1 * yr + self.z2_r;
        self.z2_r = self.b2 * r - self.a2 * yr;

        (yl, yr)
    }

    fn reset(&mut self) {
        self.z1_l = 0.0;
        self.z2_l = 0.0;
        self.z1_r = 0.0;
        self.z2_r = 0.0;
    }
}

#[derive(Clone, Copy, Debug)]
struct BiquadMono {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl BiquadMono {
    fn allpass(freq_hz: f32, q: f32, sample_rate: f32) -> Self {
        let fc = freq_hz.clamp(100.0, sample_rate * 0.45);
        let w0 = TAU * fc / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q.max(0.25));
        let a0 = 1.0 + alpha;

        Self {
            b0: (1.0 - alpha) / a0,
            b1: (-2.0 * cos) / a0,
            b2: (1.0 + alpha) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline(always)]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

// ==============================================================================
// Heap-Backed Circular Delay Line & Schroeder Reverb Building Blocks
// (Keeps SpatialDspCore under 1 KB on thread stacks)
// ==============================================================================

#[derive(Clone, Debug)]
struct FixedDelay {
    buffer: Box<[f32]>,
    write_pos: usize,
}

impl FixedDelay {
    fn new() -> Self {
        Self {
            buffer: vec![0.0; MAX_DELAY_CAPACITY].into_boxed_slice(),
            write_pos: 0,
        }
    }

    #[inline(always)]
    fn push(&mut self, sample: f32) {
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) & (MAX_DELAY_CAPACITY - 1);
    }

    #[inline(always)]
    fn read_linear(&self, delay_samples: f32) -> f32 {
        let clamped = delay_samples.clamp(0.0, (MAX_DELAY_CAPACITY - 2) as f32);
        let d0 = clamped.floor() as usize;
        let frac = clamped - d0 as f32;

        let idx0 = (self.write_pos + MAX_DELAY_CAPACITY - 1 - d0) & (MAX_DELAY_CAPACITY - 1);
        let idx1 = (self.write_pos + MAX_DELAY_CAPACITY - 2 - d0) & (MAX_DELAY_CAPACITY - 1);

        let s0 = self.buffer[idx0];
        let s1 = self.buffer[idx1];
        s0 + frac * (s1 - s0)
    }

    #[inline(always)]
    fn read_int(&self, delay_samples: usize) -> f32 {
        let d = delay_samples.min(MAX_DELAY_CAPACITY - 1);
        let idx = (self.write_pos + MAX_DELAY_CAPACITY - 1 - d) & (MAX_DELAY_CAPACITY - 1);
        self.buffer[idx]
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
    }
}

#[derive(Clone, Debug)]
struct CombFilter {
    delay: FixedDelay,
    delay_len: usize,
    feedback: f32,
    damp: f32,
    filter_store: f32,
}

impl CombFilter {
    fn new(delay_samples: usize, feedback: f32, damp: f32) -> Self {
        Self {
            delay: FixedDelay::new(),
            delay_len: delay_samples.clamp(16, MAX_DELAY_CAPACITY - 2),
            feedback: feedback.clamp(0.0, 0.92),
            damp: damp.clamp(0.05, 0.95),
            filter_store: 0.0,
        }
    }

    #[inline(always)]
    fn process(&mut self, input: f32) -> f32 {
        let output = self.delay.read_int(self.delay_len);
        self.filter_store = output * (1.0 - self.damp) + self.filter_store * self.damp;
        self.delay.push(input + self.filter_store * self.feedback);
        output
    }

    fn reset(&mut self) {
        self.delay.reset();
        self.filter_store = 0.0;
    }
}

#[derive(Clone, Debug)]
struct DelayAllPass {
    delay: FixedDelay,
    delay_len: usize,
    gain: f32,
}

impl DelayAllPass {
    fn new(delay_samples: usize, gain: f32) -> Self {
        Self {
            delay: FixedDelay::new(),
            delay_len: delay_samples.clamp(8, MAX_DELAY_CAPACITY - 2),
            gain: gain.clamp(0.1, 0.75),
        }
    }

    #[inline(always)]
    fn process(&mut self, input: f32) -> f32 {
        let buf_out = self.delay.read_int(self.delay_len);
        let output = -input + buf_out;
        self.delay.push(input + buf_out * self.gain);
        output
    }

    fn reset(&mut self) {
        self.delay.reset();
    }
}

// ==============================================================================
// Click-Free Equal-Power Crossfader
// ==============================================================================

#[derive(Clone, Debug)]
struct CrossFader {
    fade_samples: u32,
    samples_remaining: u32,
}

impl CrossFader {
    fn new(ramp: Duration, sample_rate: u32) -> Self {
        let samples = (ramp.as_secs_f64() * sample_rate as f64).round().max(1.0) as u32;
        Self {
            fade_samples: samples,
            samples_remaining: 0,
        }
    }

    fn trigger(&mut self) {
        self.samples_remaining = self.fade_samples;
    }

    #[inline(always)]
    const fn is_fading(&self) -> bool {
        self.samples_remaining > 0
    }

    #[inline(always)]
    fn next_gains(&mut self) -> (f32, f32) {
        if self.samples_remaining == 0 {
            return (0.0, 1.0);
        }
        self.samples_remaining -= 1;
        let progress = 1.0 - (self.samples_remaining as f32 / self.fade_samples as f32);
        let g_new = 0.5 * (1.0 - (progress * PI).cos());
        let g_old = 1.0 - g_new;
        (g_old, g_new)
    }
}

// ==============================================================================
// Sample-by-Sample 3D Binaural & Virtual Surround DSP Core
// ==============================================================================

struct SpatialDspCore {
    sample_rate: u32,
    bs2b_subtle_l: Bs2bFilterPair,
    bs2b_subtle_r: Bs2bFilterPair,
    bs2b_studio_l: Bs2bFilterPair,
    bs2b_studio_r: Bs2bFilterPair,
    bs2b_wide_l: Bs2bFilterPair,
    bs2b_wide_r: Bs2bFilterPair,
    // Outer-ear (pinna) & elevation HRTF filters
    pinna_presence: BiquadStereo,
    pinna_notch: BiquadStereo,
    pinna_rear_dip: BiquadStereo,
    surround_elevation: BiquadStereo,
    // 360-degree cascaded 2nd-order Biquad All-Pass phase decorrelators
    ap_l1: BiquadMono,
    ap_l2: BiquadMono,
    ap_l3: BiquadMono,
    ap_r1: BiquadMono,
    ap_r2: BiquadMono,
    ap_r3: BiquadMono,
    delay_l: FixedDelay,
    delay_r: FixedDelay,
    // 4-comb + 2-allpass Schroeder acoustic hall engine
    hall_comb_l1: CombFilter,
    hall_comb_l2: CombFilter,
    hall_comb_l3: CombFilter,
    hall_comb_l4: CombFilter,
    hall_comb_r1: CombFilter,
    hall_comb_r2: CombFilter,
    hall_comb_r3: CombFilter,
    hall_comb_r4: CombFilter,
    hall_ap_l1: DelayAllPass,
    hall_ap_l2: DelayAllPass,
    hall_ap_r1: DelayAllPass,
    hall_ap_r2: DelayAllPass,
    studio_side_hp_state: f32,
    studio_side_hp_alpha: f32,
    studio_er_lp_l: f32,
    studio_er_lp_r: f32,
    side_hp_state: f32,
    side_air_state: f32,
    side_hp_alpha: f32,
    side_air_alpha: f32,
    front_er_lp_l: f32,
    front_er_lp_r: f32,
    wide_er_lp_l: f32,
    wide_er_lp_r: f32,
    last_mode: SpatialMode,
    prev_mode: SpatialMode,
    fader: CrossFader,
    was_active: bool,
}

impl SpatialDspCore {
    fn new(sample_rate: u32) -> Self {
        let rate = sample_rate.max(8000);
        let sr = rate as f32;
        let scale = sr / 48000.0;
        let s = |base: usize| ((base as f32 * scale).round() as usize).max(16);

        Self {
            sample_rate: rate,
            // Jan Meier crossfeed (680 Hz / 7.2 dB)
            bs2b_subtle_l: Bs2bFilterPair::new(680.0, 7.2, sr),
            bs2b_subtle_r: Bs2bFilterPair::new(680.0, 7.2, sr),
            // Studio near-field monitors (750 Hz / 3.6 dB)
            bs2b_studio_l: Bs2bFilterPair::new(750.0, 3.6, sr),
            bs2b_studio_r: Bs2bFilterPair::new(750.0, 3.6, sr),
            // 3D Wide / Hall / FrontStage crossfeed (560 Hz / 3.4 dB)
            bs2b_wide_l: Bs2bFilterPair::new(560.0, 3.4, sr),
            bs2b_wide_r: Bs2bFilterPair::new(560.0, 3.4, sr),
            // Outer-ear (pinna) front-stage projection & height cues
            pinna_presence: BiquadStereo::peaking(2650.0, 4.8, 1.25, sr),
            pinna_notch: BiquadStereo::peaking(7400.0, -5.5, 2.1, sr),
            pinna_rear_dip: BiquadStereo::peaking(1150.0, -2.4, 1.4, sr),
            surround_elevation: BiquadStereo::peaking(8200.0, 3.4, 1.5, sr),
            // Orthogonal 2nd-order Biquad All-Pass cascades (creates true 3D out-of-head wrap)
            ap_l1: BiquadMono::allpass(280.0, 0.85, sr),
            ap_l2: BiquadMono::allpass(1120.0, 0.90, sr),
            ap_l3: BiquadMono::allpass(3850.0, 0.85, sr),
            ap_r1: BiquadMono::allpass(430.0, 0.85, sr),
            ap_r2: BiquadMono::allpass(1690.0, 0.90, sr),
            ap_r3: BiquadMono::allpass(5420.0, 0.85, sr),
            delay_l: FixedDelay::new(),
            delay_r: FixedDelay::new(),
            // Stereo Schroeder Concert Hall Reverb
            hall_comb_l1: CombFilter::new(s(1557), 0.81, 0.26),
            hall_comb_l2: CombFilter::new(s(1777), 0.79, 0.28),
            hall_comb_l3: CombFilter::new(s(1993), 0.77, 0.30),
            hall_comb_l4: CombFilter::new(s(2213), 0.75, 0.32),
            hall_comb_r1: CombFilter::new(s(1613), 0.81, 0.26),
            hall_comb_r2: CombFilter::new(s(1831), 0.79, 0.28),
            hall_comb_r3: CombFilter::new(s(2053), 0.77, 0.30),
            hall_comb_r4: CombFilter::new(s(2273), 0.75, 0.32),
            hall_ap_l1: DelayAllPass::new(s(241), 0.54),
            hall_ap_l2: DelayAllPass::new(s(557), 0.50),
            hall_ap_r1: DelayAllPass::new(s(263), 0.54),
            hall_ap_r2: DelayAllPass::new(s(587), 0.50),
            studio_side_hp_state: 0.0,
            studio_side_hp_alpha: (2.0 * PI * 120.0 / sr).clamp(0.001, 0.99),
            studio_er_lp_l: 0.0,
            studio_er_lp_r: 0.0,
            side_hp_state: 0.0,
            side_air_state: 0.0,
            side_hp_alpha: (2.0 * PI * 145.0 / sr).clamp(0.001, 0.99),
            side_air_alpha: (2.0 * PI * 5200.0 / sr).clamp(0.01, 0.99),
            front_er_lp_l: 0.0,
            front_er_lp_r: 0.0,
            wide_er_lp_l: 0.0,
            wide_er_lp_r: 0.0,
            last_mode: SpatialMode::Off,
            prev_mode: SpatialMode::Off,
            fader: CrossFader::new(DEFAULT_RAMP_DURATION, rate),
            was_active: false,
        }
    }

    fn set_sample_rate(&mut self, sample_rate: u32) {
        let rate = sample_rate.max(8000);
        if self.sample_rate != rate {
            *self = Self::new(rate);
        }
    }

    fn reset(&mut self) {
        self.bs2b_subtle_l.reset();
        self.bs2b_subtle_r.reset();
        self.bs2b_studio_l.reset();
        self.bs2b_studio_r.reset();
        self.bs2b_wide_l.reset();
        self.bs2b_wide_r.reset();
        self.pinna_presence.reset();
        self.pinna_notch.reset();
        self.pinna_rear_dip.reset();
        self.surround_elevation.reset();
        self.ap_l1.reset();
        self.ap_l2.reset();
        self.ap_l3.reset();
        self.ap_r1.reset();
        self.ap_r2.reset();
        self.ap_r3.reset();
        self.delay_l.reset();
        self.delay_r.reset();
        self.hall_comb_l1.reset();
        self.hall_comb_l2.reset();
        self.hall_comb_l3.reset();
        self.hall_comb_l4.reset();
        self.hall_comb_r1.reset();
        self.hall_comb_r2.reset();
        self.hall_comb_r3.reset();
        self.hall_comb_r4.reset();
        self.hall_ap_l1.reset();
        self.hall_ap_l2.reset();
        self.hall_ap_r1.reset();
        self.hall_ap_r2.reset();
        self.studio_side_hp_state = 0.0;
        self.studio_er_lp_l = 0.0;
        self.studio_er_lp_r = 0.0;
        self.side_hp_state = 0.0;
        self.side_air_state = 0.0;
        self.front_er_lp_l = 0.0;
        self.front_er_lp_r = 0.0;
        self.wide_er_lp_l = 0.0;
        self.wide_er_lp_r = 0.0;
        self.was_active = false;
    }

    #[inline]
    fn process_stereo(&mut self, l: f32, r: f32, mode: SpatialMode) -> (f32, f32) {
        self.was_active = true;
        if mode != self.last_mode {
            self.prev_mode = self.last_mode;
            self.last_mode = mode;
            self.fader.trigger();
        }

        self.delay_l.push(l);
        self.delay_r.push(r);

        let (g_old, g_new) = self.fader.next_gains();
        let blending = g_old > 0.0001;

        let need_subtle =
            mode == SpatialMode::Subtle || (blending && self.prev_mode == SpatialMode::Subtle);
        let need_studio =
            mode == SpatialMode::Studio || (blending && self.prev_mode == SpatialMode::Studio);
        let need_front = mode == SpatialMode::FrontStage
            || (blending && self.prev_mode == SpatialMode::FrontStage);
        let need_wide_group = matches!(
            mode,
            SpatialMode::FrontStage | SpatialMode::WideStage | SpatialMode::ConcertHall
        ) || (blending
            && matches!(
                self.prev_mode,
                SpatialMode::FrontStage | SpatialMode::WideStage | SpatialMode::ConcertHall
            ));
        let need_wide_er = mode == SpatialMode::WideStage
            || (blending && self.prev_mode == SpatialMode::WideStage);
        let need_hall = mode == SpatialMode::ConcertHall
            || (blending && self.prev_mode == SpatialMode::ConcertHall);

        let sr = self.sample_rate as f32;
        let itd_samples = 0.00042 * sr;
        let del_r = self.delay_r.read_linear(itd_samples);
        let del_l = self.delay_l.read_linear(itd_samples);

        let subtle_out = if need_subtle {
            let sb_l =
                self.bs2b_subtle_l.process_direct(l) + self.bs2b_subtle_l.process_cross(del_r);
            let sb_r =
                self.bs2b_subtle_r.process_direct(r) + self.bs2b_subtle_r.process_cross(del_l);
            (sb_l, sb_r)
        } else {
            (0.0, 0.0)
        };

        let studio_out = if need_studio {
            let st_l =
                self.bs2b_studio_l.process_direct(l) + self.bs2b_studio_l.process_cross(del_r);
            let st_r =
                self.bs2b_studio_r.process_direct(r) + self.bs2b_studio_r.process_cross(del_l);
            let mid = (st_l + st_r) * 0.5;
            let raw_side = (st_l - st_r) * 0.5;
            self.studio_side_hp_state +=
                self.studio_side_hp_alpha * (raw_side - self.studio_side_hp_state);
            let tight_side = (raw_side - self.studio_side_hp_state) * 1.40;

            let s1 = (0.007 * sr) as usize;
            let s2 = (0.013 * sr) as usize;
            let alpha = (2.0 * PI * 3000.0 / sr).clamp(0.01, 0.99);
            let raw_er_l = 0.65 * self.delay_r.read_int(s1) + 0.35 * self.delay_l.read_int(s2);
            let raw_er_r = 0.65 * self.delay_l.read_int(s1) + 0.35 * self.delay_r.read_int(s2);
            self.studio_er_lp_l += alpha * (raw_er_l - self.studio_er_lp_l);
            self.studio_er_lp_r += alpha * (raw_er_r - self.studio_er_lp_r);

            let wet = 0.14;
            (
                (mid + tight_side + self.studio_er_lp_l * wet) * 0.90,
                (mid - tight_side + self.studio_er_lp_r * wet) * 0.90,
            )
        } else {
            (0.0, 0.0)
        };

        let (cf_l, cf_r) = if need_wide_group {
            (
                self.bs2b_wide_l.process_direct(l) + self.bs2b_wide_l.process_cross(del_r),
                self.bs2b_wide_r.process_direct(r) + self.bs2b_wide_r.process_cross(del_l),
            )
        } else {
            (0.0, 0.0)
        };

        let (front_shaped_side, wide_shaped_side, hall_shaped_side) = if need_wide_group {
            let raw_side = (cf_l - cf_r) * 0.5;
            self.side_hp_state += self.side_hp_alpha * (raw_side - self.side_hp_state);
            let clean_side = raw_side - self.side_hp_state;
            self.side_air_state += self.side_air_alpha * (clean_side - self.side_air_state);
            let high_side = clean_side - self.side_air_state;

            // Cascaded 2nd-order Biquad All-Pass phase rotators create true 360° wrap-around
            let ap_l = self.ap_l3.process(self.ap_l2.process(self.ap_l1.process(clean_side)));
            let ap_r = self.ap_r3.process(self.ap_r2.process(self.ap_r1.process(clean_side)));
            let diffused_side = (ap_l - ap_r) * 0.5;

            (
                (clean_side * 1.45) + (diffused_side * 0.45) + (high_side * 0.35),
                (clean_side * 2.05) + (diffused_side * 0.85) + (high_side * 0.58),
                (clean_side * 1.80) + (diffused_side * 0.72) + (high_side * 0.52),
            )
        } else {
            (0.0, 0.0, 0.0)
        };

        let front_out = if need_front {
            let mid = (cf_l + cf_r) * 0.60;
            let (p_l, p_r) = self
                .pinna_presence
                .process_stereo(mid + front_shaped_side, mid - front_shaped_side);
            let (n_l, n_r) = self.pinna_notch.process_stereo(p_l, p_r);
            let (hrtf_l, hrtf_r) = self.pinna_rear_dip.process_stereo(n_l, n_r);

            let f1 = (0.011 * sr) as usize;
            let f2 = (0.019 * sr) as usize;
            let alpha = (2.0 * PI * 3600.0 / sr).clamp(0.01, 0.99);

            let raw_er_l = 0.62 * self.delay_r.read_int(f1) + 0.38 * self.delay_l.read_int(f2);
            let raw_er_r = 0.62 * self.delay_l.read_int(f1) + 0.38 * self.delay_r.read_int(f2);
            self.front_er_lp_l += alpha * (raw_er_l - self.front_er_lp_l);
            self.front_er_lp_r += alpha * (raw_er_r - self.front_er_lp_r);

            let wet = 0.19;
            (
                (hrtf_l + self.front_er_lp_l * wet) * 0.82,
                (hrtf_r + self.front_er_lp_r * wet) * 0.82,
            )
        } else {
            (0.0, 0.0)
        };

        if need_wide_er {
            // Virtual 7.1 side & rear surround early reflection taps (15ms - 48ms)
            let t1 = (0.015 * sr) as usize;
            let t2 = (0.024 * sr) as usize;
            let t3 = (0.035 * sr) as usize;
            let t4 = (0.048 * sr) as usize;
            let alpha = (2.0 * PI * 4200.0 / sr).clamp(0.01, 0.99);

            let raw_er_l = 0.45 * self.delay_r.read_int(t1)
                - 0.30 * self.delay_l.read_int(t2)
                + 0.24 * self.delay_r.read_int(t3)
                - 0.16 * self.delay_l.read_int(t4);
            self.wide_er_lp_l += alpha * (raw_er_l - self.wide_er_lp_l);

            let raw_er_r = 0.45 * self.delay_l.read_int(t1)
                - 0.30 * self.delay_r.read_int(t2)
                + 0.24 * self.delay_l.read_int(t3)
                - 0.16 * self.delay_r.read_int(t4);
            self.wide_er_lp_r += alpha * (raw_er_r - self.wide_er_lp_r);
        }

        let hall_reverb = if need_hall {
            let in_l = l * 0.7 + r * 0.3;
            let in_r = r * 0.7 + l * 0.3;

            let sum_l = self.hall_comb_l1.process(in_l)
                + self.hall_comb_l2.process(in_l)
                + self.hall_comb_l3.process(in_l)
                + self.hall_comb_l4.process(in_l);
            let sum_r = self.hall_comb_r1.process(in_r)
                + self.hall_comb_r2.process(in_r)
                + self.hall_comb_r3.process(in_r)
                + self.hall_comb_r4.process(in_r);

            let diff_l = self.hall_ap_l2.process(self.hall_ap_l1.process(sum_l * 0.25));
            let diff_r = self.hall_ap_r2.process(self.hall_ap_r1.process(sum_r * 0.25));
            (diff_l, diff_r)
        } else {
            (0.0, 0.0)
        };

        let mut eval = |m: SpatialMode| -> (f32, f32) {
            match m {
                SpatialMode::Off => (l, r),
                SpatialMode::Subtle => subtle_out,
                SpatialMode::Studio => studio_out,
                SpatialMode::FrontStage => front_out,
                SpatialMode::WideStage => {
                    let mid = (cf_l + cf_r) * 0.48;
                    let er_wet = 0.28;
                    let (el_l, el_r) = self.surround_elevation.process_stereo(
                        (mid + wide_shaped_side) + self.wide_er_lp_l * er_wet,
                        (mid - wide_shaped_side) + self.wide_er_lp_r * er_wet,
                    );
                    (el_l * 0.76, el_r * 0.76)
                }
                SpatialMode::ConcertHall => {
                    let mid = (cf_l + cf_r) * 0.48;
                    let hall_wet = 0.44;
                    (
                        ((mid + hall_shaped_side) + hall_reverb.0 * hall_wet) * 0.72,
                        ((mid - hall_shaped_side) + hall_reverb.1 * hall_wet) * 0.72,
                    )
                }
            }
        };

        let (new_l, new_r) = eval(mode);
        if blending {
            let (old_l, old_r) = eval(self.prev_mode);
            (
                (old_l * g_old + new_l * g_new).clamp(-0.99, 0.99),
                (old_r * g_old + new_r * g_new).clamp(-0.99, 0.99),
            )
        } else {
            (new_l.clamp(-0.99, 0.99), new_r.clamp(-0.99, 0.99))
        }
    }
}

// ==============================================================================
// Thread-Safe Spatial Audio Processor
// ==============================================================================

struct SharedHandle {
    enabled: AtomicBool,
    mode_id: AtomicU32,
    sample_rate: AtomicU32,
}

#[derive(Clone)]
pub struct SpatialAudioProcessor {
    shared: Arc<SharedHandle>,
}

impl SpatialAudioProcessor {
    pub fn new(enabled: bool, mode: SpatialMode) -> Self {
        Self::with_sample_rate(enabled, mode, 48000)
    }

    pub fn with_sample_rate(enabled: bool, mode: SpatialMode, sample_rate: u32) -> Self {
        Self {
            shared: Arc::new(SharedHandle {
                enabled: AtomicBool::new(enabled),
                mode_id: AtomicU32::new(mode as u32),
                sample_rate: AtomicU32::new(sample_rate.max(8000)),
            }),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::Acquire)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.shared.enabled.store(enabled, Ordering::Release);
    }

    pub fn mode(&self) -> SpatialMode {
        SpatialMode::from_u32(self.shared.mode_id.load(Ordering::Acquire))
    }

    pub fn set_mode(&self, mode: SpatialMode) {
        self.shared.mode_id.store(mode as u32, Ordering::Release);
    }

    #[inline]
    pub fn active_mode(&self) -> SpatialMode {
        if !self.is_enabled() {
            SpatialMode::Off
        } else {
            self.mode()
        }
    }

    pub const fn wasapi_stream_flags(&self) -> u32 {
        0
    }

    pub fn prepare(&self, sample_rate: u32) -> Result<(), AudioError> {
        if sample_rate > 0 {
            self.shared.sample_rate.store(sample_rate, Ordering::Release);
        }
        Ok(())
    }

    pub fn wrap_source<I>(&self, source: I) -> SpatialSource<I>
    where
        I: Source<Item = f32>,
    {
        SpatialSource::new(source, self.clone())
    }

    pub fn should_process(&self) -> bool {
        self.active_mode() != SpatialMode::Off
    }
}

impl Default for SpatialAudioProcessor {
    fn default() -> Self {
        Self::new(false, SpatialMode::Off)
    }
}

// ==============================================================================
// Rodio Source Adapter (1-to-1 Sample-Synchronized Pipeline)
// ==============================================================================

pub struct SpatialSource<I> {
    input: I,
    processor: SpatialAudioProcessor,
    core: Box<SpatialDspCore>,
    channels: u16,
    sample_rate: u32,
    channel_idx: u16,
    pending_right: Option<f32>,
}

impl<I: Source<Item = f32>> SpatialSource<I> {
    pub fn new(input: I, processor: SpatialAudioProcessor) -> Self {
        let channels = input.channels().get();
        let sample_rate = input.sample_rate().get();
        Self {
            input,
            processor,
            core: Box::new(SpatialDspCore::new(sample_rate)),
            channels,
            sample_rate,
            channel_idx: 0,
            pending_right: None,
        }
    }

    pub const fn processor(&self) -> &SpatialAudioProcessor {
        &self.processor
    }
}

impl<I: Source<Item = f32>> Iterator for SpatialSource<I> {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(right) = self.pending_right.take() {
            self.channel_idx = 0;
            return Some(right);
        }

        let sample = self.input.next()?;

        if self.channel_idx == 0 {
            let cur_ch = self.input.channels().get();
            let cur_sr = self.input.sample_rate().get();
            if cur_ch != self.channels || cur_sr != self.sample_rate {
                self.channels = cur_ch;
                self.sample_rate = cur_sr;
                self.core.set_sample_rate(cur_sr);
            }
        }

        let mode = self.processor.active_mode();
        if self.channels != 2 {
            self.channel_idx = if self.channels > 0 {
                (self.channel_idx + 1) % self.channels
            } else {
                0
            };
            if self.core.was_active {
                self.core.reset();
                self.core.last_mode = SpatialMode::Off;
            }
            return Some(sample);
        }

        let Some(right_sample) = self.input.next() else {
            self.channel_idx = 0;
            return Some(sample);
        };

        if mode == SpatialMode::Off && !self.core.was_active {
            self.pending_right = Some(right_sample);
            self.channel_idx = 1;
            return Some(sample);
        }

        let (out_l, out_r) = self.core.process_stereo(sample, right_sample, mode);
        if mode == SpatialMode::Off
            && self.core.last_mode == SpatialMode::Off
            && !self.core.fader.is_fading()
        {
            self.core.reset();
        }

        self.pending_right = Some(out_r);
        self.channel_idx = 1;
        Some(out_l)
    }
}

impl<I: Source<Item = f32>> Source for SpatialSource<I> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> NonZero<u16> {
        self.input.channels()
    }

    fn sample_rate(&self) -> NonZero<u32> {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        self.channel_idx = 0;
        self.pending_right = None;
        self.core.reset();
        self.input.try_seek(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    #[test]
    fn stereo_phase_symmetry_on_mono_input() {
        let mut core = SpatialDspCore::new(48_000);
        for mode in SpatialMode::ALL {
            core.reset();
            core.last_mode = mode;
            for i in 0..2048 {
                let s = ((i as f32 * 0.05).sin()) * 0.4;
                let (out_l, out_r) = core.process_stereo(s, s, mode);
                assert!(
                    (out_l - out_r).abs() < 1e-6,
                    "mode={mode:?} sample={i}: L ({out_l}) != R ({out_r})"
                );
            }
        }
    }

    #[test]
    fn click_free_transition_to_off() {
        let input_samples = vec![0.5_f32; 9600];
        let buffer = SamplesBuffer::new(
            NonZero::new(2).unwrap(),
            NonZero::new(48_000).unwrap(),
            input_samples,
        );
        let processor =
            SpatialAudioProcessor::with_sample_rate(true, SpatialMode::ConcertHall, 48_000);
        let mut source = processor.wrap_source(buffer);

        let mut prev_l = 0.0;
        for _ in 0..1000 {
            prev_l = source.next().unwrap();
            let _ = source.next().unwrap();
        }

        processor.set_enabled(false);
        for _ in 0..3000 {
            let l = source.next().unwrap();
            let _ = source.next().unwrap();
            assert!(
                (l - prev_l).abs() < 0.02,
                "Kapatma geçişinde ani sıçrama tespit edildi: prev={prev_l}, cur={l}"
            );
            prev_l = l;
        }
    }
}