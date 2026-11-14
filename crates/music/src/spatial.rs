//! Spatial Audio / 3D Soundstage Core Processing Module.
//!
//! Built on complementary 1st-order bilinear `libbs2b` (Bauer Stereophonic-to-Binaural)
//! crossfeed filters (`H_direct(z) + H_cross(z) == 1.0`), phase-coherent Mid/Side
//! soundstage expansion with sub-bass mono anchoring, distinct acoustic room models,
//! and 1-to-1 sample-accurate `rodio::Source` passthrough.

use std::f32::consts::PI;
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

const MAX_DELAY_CAPACITY: usize = 8192;
const DEFAULT_RAMP_DURATION: Duration = Duration::from_millis(35);

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
    Studio = 1,
    WideStage = 2,
    ConcertHall = 3,
}

impl SpatialMode {
    pub const ALL: [Self; 4] = [Self::Off, Self::Studio, Self::WideStage, Self::ConcertHall];

    #[inline]
    pub const fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Studio,
            2 => Self::WideStage,
            3 => Self::ConcertHall,
            _ => Self::Off,
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Studio => "studio",
            Self::WideStage => "wide_stage",
            Self::ConcertHall => "concert_hall",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Studio => "Studio (Intimate)",
            Self::WideStage => "Wide Stage",
            Self::ConcertHall => "Concert Hall",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Off => "Bit-exact passthrough with zero DSP processing",
            Self::Studio => "Bauer bs2b crossfeed simulating near-field studio monitors",
            Self::WideStage => "Phase-coherent stereo soundstage expansion with centered vocals",
            Self::ConcertHall => "Wide stage with warm acoustic concert hall reflections",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "off" => Some(Self::Off),
            "studio" => Some(Self::Studio),
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
            Self::Studio => SpatialParams {
                width: 0.35,
                depth: 0.25,
                crossfeed_gain: 0.45,
                hrtf_gain: 0.25,
                reflection_gain: 0.0,
            },
            Self::WideStage => SpatialParams {
                width: 0.75,
                depth: 0.55,
                crossfeed_gain: 0.22,
                hrtf_gain: 0.55,
                reflection_gain: 0.08,
            },
            Self::ConcertHall => SpatialParams {
                width: 0.82,
                depth: 0.85,
                crossfeed_gain: 0.28,
                hrtf_gain: 0.72,
                reflection_gain: 0.35,
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
// Zero-Allocation Circular Delay Line
// ==============================================================================

#[derive(Clone, Debug)]
struct FixedDelay {
    buffer: [f32; MAX_DELAY_CAPACITY],
    write_pos: usize,
}

impl FixedDelay {
    const fn new() -> Self {
        Self {
            buffer: [0.0; MAX_DELAY_CAPACITY],
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
// Sample-by-Sample Stereo DSP Core (1-to-1 Rodio Sync)
// ==============================================================================

struct SpatialDspCore {
    sample_rate: u32,
    bs2b_studio_l: Bs2bFilterPair,
    bs2b_studio_r: Bs2bFilterPair,
    bs2b_wide_l: Bs2bFilterPair,
    bs2b_wide_r: Bs2bFilterPair,
    delay_l: FixedDelay,
    delay_r: FixedDelay,
    hall_fb_l: FixedDelay,
    hall_fb_r: FixedDelay,
    studio_side_hp_state: f32,
    studio_side_hp_alpha: f32,
    side_hp_state: f32,
    side_air_state: f32,
    side_hp_alpha: f32,
    side_air_alpha: f32,
    wide_er_lp_l: f32,
    wide_er_lp_r: f32,
    hall_er_lp_l: f32,
    hall_er_lp_r: f32,
    last_mode: SpatialMode,
    prev_mode: SpatialMode,
    fader: CrossFader,
    was_active: bool,
}

impl SpatialDspCore {
    fn new(sample_rate: u32) -> Self {
        let rate = sample_rate.max(8000);
        let sr = rate as f32;
        Self {
            sample_rate: rate,
            bs2b_studio_l: Bs2bFilterPair::new(680.0, 4.8, sr),
            bs2b_studio_r: Bs2bFilterPair::new(680.0, 4.8, sr),
            bs2b_wide_l: Bs2bFilterPair::new(480.0, 3.8, sr),
            bs2b_wide_r: Bs2bFilterPair::new(480.0, 3.8, sr),
            delay_l: FixedDelay::new(),
            delay_r: FixedDelay::new(),
            hall_fb_l: FixedDelay::new(),
            hall_fb_r: FixedDelay::new(),
            studio_side_hp_state: 0.0,
            studio_side_hp_alpha: (2.0 * PI * 100.0 / sr).clamp(0.001, 0.99),
            side_hp_state: 0.0,
            side_air_state: 0.0,
            side_hp_alpha: (2.0 * PI * 140.0 / sr).clamp(0.001, 0.99),
            side_air_alpha: (2.0 * PI * 6500.0 / sr).clamp(0.01, 0.99),
            wide_er_lp_l: 0.0,
            wide_er_lp_r: 0.0,
            hall_er_lp_l: 0.0,
            hall_er_lp_r: 0.0,
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
        self.bs2b_studio_l.reset();
        self.bs2b_studio_r.reset();
        self.bs2b_wide_l.reset();
        self.bs2b_wide_r.reset();
        self.delay_l.reset();
        self.delay_r.reset();
        self.hall_fb_l.reset();
        self.hall_fb_r.reset();
        self.studio_side_hp_state = 0.0;
        self.side_hp_state = 0.0;
        self.side_air_state = 0.0;
        self.wide_er_lp_l = 0.0;
        self.wide_er_lp_r = 0.0;
        self.hall_er_lp_l = 0.0;
        self.hall_er_lp_r = 0.0;
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

        let need_studio =
            mode == SpatialMode::Studio || (blending && self.prev_mode == SpatialMode::Studio);
        let need_wide_or_hall = matches!(mode, SpatialMode::WideStage | SpatialMode::ConcertHall)
            || (blending
                && matches!(
                    self.prev_mode,
                    SpatialMode::WideStage | SpatialMode::ConcertHall
                ));
        let need_wide_er = mode == SpatialMode::WideStage
            || (blending && self.prev_mode == SpatialMode::WideStage);
        let need_hall_er = mode == SpatialMode::ConcertHall
            || (blending && self.prev_mode == SpatialMode::ConcertHall);

        let sr = self.sample_rate as f32;
        let itd_samples = 0.00025 * sr;
        let del_r = self.delay_r.read_linear(itd_samples);
        let del_l = self.delay_l.read_linear(itd_samples);

        let studio_out = if need_studio {
            let st_l =
                self.bs2b_studio_l.process_direct(l) + self.bs2b_studio_l.process_cross(del_r);
            let st_r =
                self.bs2b_studio_r.process_direct(r) + self.bs2b_studio_r.process_cross(del_l);
            let mid = (st_l + st_r) * 0.5;
            let raw_side = (st_l - st_r) * 0.5;
            self.studio_side_hp_state +=
                self.studio_side_hp_alpha * (raw_side - self.studio_side_hp_state);
            let tight_side = (raw_side - self.studio_side_hp_state) * 1.05;
            (mid + tight_side, mid - tight_side)
        } else {
            (0.0, 0.0)
        };

        let (cf_l, cf_r) = if need_wide_or_hall {
            (
                self.bs2b_wide_l.process_direct(l) + self.bs2b_wide_l.process_cross(del_r),
                self.bs2b_wide_r.process_direct(r) + self.bs2b_wide_r.process_cross(del_l),
            )
        } else {
            (0.0, 0.0)
        };

        let (wide_shaped_side, hall_shaped_side) = if need_wide_or_hall {
            let raw_side = (cf_l - cf_r) * 0.5;
            self.side_hp_state += self.side_hp_alpha * (raw_side - self.side_hp_state);
            let clean_side = raw_side - self.side_hp_state;
            self.side_air_state += self.side_air_alpha * (clean_side - self.side_air_state);
            let high_side = clean_side - self.side_air_state;
            (
                (clean_side * 1.38) + (high_side * 0.18),
                (clean_side * 1.28) + (high_side * 0.22),
            )
        } else {
            (0.0, 0.0)
        };

        if need_wide_er {
            let t1 = (0.014 * sr) as usize;
            let t2 = (0.021 * sr) as usize;
            let t3 = (0.031 * sr) as usize;
            let t4 = (0.041 * sr) as usize;
            let alpha = (2.0 * PI * 2800.0 / sr).clamp(0.01, 0.99);

            let raw_er_l = 0.40 * self.delay_r.read_int(t1)
                + 0.28 * self.delay_l.read_int(t2)
                + 0.20 * self.delay_r.read_int(t3)
                + 0.12 * self.delay_l.read_int(t4);
            self.wide_er_lp_l += alpha * (raw_er_l - self.wide_er_lp_l);

            let raw_er_r = 0.40 * self.delay_l.read_int(t1)
                + 0.28 * self.delay_r.read_int(t2)
                + 0.20 * self.delay_l.read_int(t3)
                + 0.12 * self.delay_r.read_int(t4);
            self.wide_er_lp_r += alpha * (raw_er_r - self.wide_er_lp_r);
        }

        if need_hall_er {
            let h1 = (0.028 * sr) as usize;
            let h2 = (0.043 * sr) as usize;
            let h3 = (0.061 * sr) as usize;
            let h4 = (0.079 * sr) as usize;
            let tail_tap = (0.067 * sr) as usize;
            let warm_alpha = (2.0 * PI * 3400.0 / sr).clamp(0.01, 0.99);

            let prev_tail_l = self.hall_fb_r.read_int(tail_tap) * 0.28;
            let prev_tail_r = self.hall_fb_l.read_int(tail_tap) * 0.28;

            let raw_hall_l = 0.36 * self.delay_r.read_int(h1)
                + 0.28 * self.delay_l.read_int(h2)
                + 0.22 * self.delay_r.read_int(h3)
                + 0.14 * self.delay_l.read_int(h4)
                + prev_tail_l;
            self.hall_er_lp_l += warm_alpha * (raw_hall_l - self.hall_er_lp_l);

            let raw_hall_r = 0.36 * self.delay_l.read_int(h1)
                + 0.28 * self.delay_r.read_int(h2)
                + 0.22 * self.delay_l.read_int(h3)
                + 0.14 * self.delay_r.read_int(h4)
                + prev_tail_r;
            self.hall_er_lp_r += warm_alpha * (raw_hall_r - self.hall_er_lp_r);

            self.hall_fb_l.push(self.hall_er_lp_l);
            self.hall_fb_r.push(self.hall_er_lp_r);
        }

        let eval = |m: SpatialMode| -> (f32, f32) {
            match m {
                SpatialMode::Off => (l, r),
                SpatialMode::Studio => studio_out,
                SpatialMode::WideStage => {
                    let mid = (cf_l + cf_r) * 0.5;
                    let er_wet = 0.022;
                    (
                        ((mid + wide_shaped_side) + self.wide_er_lp_l * er_wet) * 0.91,
                        ((mid - wide_shaped_side) + self.wide_er_lp_r * er_wet) * 0.91,
                    )
                }
                SpatialMode::ConcertHall => {
                    let mid = (cf_l + cf_r) * 0.5;
                    let hall_wet = 0.105;
                    (
                        ((mid + hall_shaped_side) + self.hall_er_lp_l * hall_wet) * 0.89,
                        ((mid - hall_shaped_side) + self.hall_er_lp_r * hall_wet) * 0.89,
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
    core: SpatialDspCore,
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
            core: SpatialDspCore::new(sample_rate),
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