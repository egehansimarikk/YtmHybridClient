//! Spatial Audio / 3D Soundstage Core Processing Module.
//!
//! Complementary bilinear `libbs2b` (Bauer Stereophonic-to-Binaural) crossfeed
//! (`H_direct(z) + H_cross(z) == 1.0`), phase-coherent Mid/Side soundstage
//! expansion, and sample-accurate 1-to-1 `rodio::Source` passthrough.

use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

const MAX_DELAY_CAPACITY: usize = 4096;
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
    InvalidParameter(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeviceUnavailable(msg) => write!(f, "Audio device unavailable: {msg}"),
            Self::UnsupportedFormat { channels, sample_rate } => {
                write!(f, "Unsupported format: {channels} channels, {sample_rate} Hz")
            }
            Self::BufferUnderflow => write!(f, "Audio buffer underflow"),
            Self::ChannelMismatch { expected, found } => {
                write!(f, "Channel mismatch: expected {expected}, found {found}")
            }
            Self::InvalidParameter(msg) => write!(f, "Invalid parameter: {msg}"),
        }
    }
}

impl std::error::Error for AudioError {}

// ==============================================================================
// Spatial Audio Modes
// ==============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum SpatialMode {
    #[default]
    Off = 0,
    /// Gentle Bauer bs2b (700 Hz / 6.0 dB) near-field studio monitor simulation.
    Studio = 1,
    /// Light bs2b crossfeed + phase-coherent Mid/Side stereo stage expansion.
    WideStage = 2,
    /// Wide stage + subtle warm damped early room reflections.
    ConcertHall = 3,
}

impl SpatialMode {
    pub const ALL: [Self; 4] = [
        Self::Off,
        Self::Studio,
        Self::WideStage,
        Self::ConcertHall,
    ];

    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Studio,
            2 => Self::WideStage,
            3 => Self::ConcertHall,
            _ => Self::Off,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Studio => "studio",
            Self::WideStage => "wide_stage",
            Self::ConcertHall => "concert_hall",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Studio => "Studio (Intimate)",
            Self::WideStage => "Wide Stage",
            Self::ConcertHall => "Concert Hall",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Off => "Bit-exact bypass without any DSP processing.",
            Self::Studio => "Bauer bs2b acoustic crossfeed simulating near-field studio monitors.",
            Self::WideStage => "Expands stereo soundstage width while keeping vocals centered.",
            Self::ConcertHall => "Adds subtle warm room early reflections for live concert depth.",
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
// Zero-Allocation Stack Circular Delay Line
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
// Equal-Power Crossfader
// ==============================================================================

#[derive(Clone, Debug)]
pub struct CrossFader {
    fade_samples: u32,
    samples_remaining: u32,
}

impl CrossFader {
    pub fn new(ramp: Duration, sample_rate: u32) -> Self {
        let samples = (ramp.as_secs_f64() * sample_rate as f64 * 2.0).round().max(1.0) as u32;
        Self {
            fade_samples: samples,
            samples_remaining: 0,
        }
    }

    pub fn trigger(&mut self) {
        self.samples_remaining = self.fade_samples;
    }

    #[inline(always)]
    pub fn next_gains(&mut self) -> (f32, f32) {
        if self.samples_remaining == 0 {
            return (0.0, 1.0);
        }
        let progress = 1.0 - (self.samples_remaining as f32 / self.fade_samples as f32);
        self.samples_remaining -= 1;
        let angle = progress * (PI * 0.5);
        (angle.cos(), angle.sin())
    }
}

// ==============================================================================
// Sample-by-Sample Stereo DSP Engine (1-to-1 Rodio Sync)
// ==============================================================================

struct SpatialDspCore {
    sample_rate: f32,
    bs2b_studio_l: Bs2bFilterPair,
    bs2b_studio_r: Bs2bFilterPair,
    bs2b_wide_l: Bs2bFilterPair,
    bs2b_wide_r: Bs2bFilterPair,
    delay_l: FixedDelay,
    delay_r: FixedDelay,
    last_cf_l: f32,
    last_cf_r: f32,
    er_lp_l: f32,
    er_lp_r: f32,
    last_mode: SpatialMode,
    prev_mode: SpatialMode,
    fader: CrossFader,
    was_active: bool,
}

impl SpatialDspCore {
    fn new(sample_rate: u32) -> Self {
        let sr = sample_rate.max(8000) as f32;
        Self {
            sample_rate: sr,
            // Studio: Gentle libbs2b (700 Hz, 6.0 dB feed) - transparent on IEMs
            bs2b_studio_l: Bs2bFilterPair::new(700.0, 6.0, sr),
            bs2b_studio_r: Bs2bFilterPair::new(700.0, 6.0, sr),
            // Wide / Hall: Subtle crossfeed (750 Hz, 7.5 dB feed) to preserve stereo width
            bs2b_wide_l: Bs2bFilterPair::new(750.0, 7.5, sr),
            bs2b_wide_r: Bs2bFilterPair::new(750.0, 7.5, sr),
            delay_l: FixedDelay::new(),
            delay_r: FixedDelay::new(),
            last_cf_l: 0.0,
            last_cf_r: 0.0,
            er_lp_l: 0.0,
            er_lp_r: 0.0,
            last_mode: SpatialMode::Off,
            prev_mode: SpatialMode::Off,
            fader: CrossFader::new(DEFAULT_RAMP_DURATION, sample_rate.max(8000)),
            was_active: false,
        }
    }

    fn reset(&mut self) {
        self.bs2b_studio_l.reset();
        self.bs2b_studio_r.reset();
        self.bs2b_wide_l.reset();
        self.bs2b_wide_r.reset();
        self.delay_l.reset();
        self.delay_r.reset();
        self.last_cf_l = 0.0;
        self.last_cf_r = 0.0;
        self.er_lp_l = 0.0;
        self.er_lp_r = 0.0;
        self.was_active = false;
    }

    #[inline]
    fn render_channel(&mut self, sample: f32, is_right: bool, mode: SpatialMode) -> f32 {
        let itd_samples = 0.00025 * self.sample_rate; // ~250 us interaural delay

        match mode {
            SpatialMode::Off => sample,

            SpatialMode::Studio => {
                if !is_right {
                    let del_r = self.delay_r.read_linear(itd_samples);
                    self.bs2b_studio_l.process_direct(sample) + self.bs2b_studio_l.process_cross(del_r)
                } else {
                    let del_l = self.delay_l.read_linear(itd_samples);
                    self.bs2b_studio_r.process_direct(sample) + self.bs2b_studio_r.process_cross(del_l)
                }
            }

            SpatialMode::WideStage => {
                if !is_right {
                    let del_r = self.delay_r.read_linear(itd_samples);
                    let cf_l = self.bs2b_wide_l.process_direct(sample) + self.bs2b_wide_l.process_cross(del_r);
                    self.last_cf_l = cf_l;
                    let mid = (cf_l + self.last_cf_r) * 0.5;
                    let side = (cf_l - self.last_cf_r) * 0.5 * 1.18;
                    (mid + side) * 0.945
                } else {
                    let del_l = self.delay_l.read_linear(itd_samples);
                    let cf_r = self.bs2b_wide_r.process_direct(sample) + self.bs2b_wide_r.process_cross(del_l);
                    self.last_cf_r = cf_r;
                    let mid = (self.last_cf_l + cf_r) * 0.5;
                    let side = (self.last_cf_l - cf_r) * 0.5 * 1.18;
                    (mid - side) * 0.945
                }
            }

            SpatialMode::ConcertHall => {
                let t1 = (0.014 * self.sample_rate) as usize;
                let t2 = (0.021 * self.sample_rate) as usize;
                let t3 = (0.031 * self.sample_rate) as usize;
                let t4 = (0.041 * self.sample_rate) as usize;
                let alpha = (2.0 * PI * 2800.0 / self.sample_rate).clamp(0.01, 0.99);
                let wet = 0.055;

                if !is_right {
                    let del_r = self.delay_r.read_linear(itd_samples);
                    let cf_l = self.bs2b_wide_l.process_direct(sample) + self.bs2b_wide_l.process_cross(del_r);
                    self.last_cf_l = cf_l;
                    let mid = (cf_l + self.last_cf_r) * 0.5;
                    let side = (cf_l - self.last_cf_r) * 0.5 * 1.24;
                    let stage_l = mid + side;

                    let raw_er_l = 0.40 * self.delay_r.read_int(t1)
                        + 0.28 * self.delay_l.read_int(t2)
                        + 0.20 * self.delay_r.read_int(t3)
                        + 0.12 * self.delay_l.read_int(t4);
                    self.er_lp_l += alpha * (raw_er_l - self.er_lp_l);

                    (stage_l + self.er_lp_l * wet) * 0.915
                } else {
                    let del_l = self.delay_l.read_linear(itd_samples);
                    let cf_r = self.bs2b_wide_r.process_direct(sample) + self.bs2b_wide_r.process_cross(del_l);
                    self.last_cf_r = cf_r;
                    let mid = (self.last_cf_l + cf_r) * 0.5;
                    let side = (self.last_cf_l - cf_r) * 0.5 * 1.24;
                    let stage_r = mid - side;

                    let raw_er_r = 0.40 * self.delay_l.read_int(t1)
                        + 0.28 * self.delay_r.read_int(t2)
                        + 0.20 * self.delay_l.read_int(t3)
                        + 0.12 * self.delay_r.read_int(t4);
                    self.er_lp_r += alpha * (raw_er_r - self.er_lp_r);

                    (stage_r + self.er_lp_r * wet) * 0.915
                }
            }
        }
    }

    #[inline]
    fn process_sample(&mut self, sample: f32, is_right: bool, mode: SpatialMode) -> f32 {
        self.was_active = true;

        if mode != self.last_mode {
            self.prev_mode = self.last_mode;
            self.last_mode = mode;
            self.fader.trigger();
        }

        if !is_right {
            self.delay_l.push(sample);
        } else {
            self.delay_r.push(sample);
        }

        let (g_old, g_new) = self.fader.next_gains();
        let new_out = self.render_channel(sample, is_right, mode);

        if g_old > 0.0001 {
            let old_out = self.render_channel(sample, is_right, self.prev_mode);
            (old_out * g_old + new_out * g_new).clamp(-0.99, 0.99)
        } else {
            new_out.clamp(-0.99, 0.99)
        }
    }
}

// ==============================================================================
// Thread-Safe Processor Handle & Rodio Source Wrapper
// ==============================================================================

struct SharedSpatialState {
    enabled: AtomicBool,
    mode: AtomicU8,
}

#[derive(Clone)]
pub struct SpatialAudioProcessor {
    state: Arc<SharedSpatialState>,
}

impl Default for SpatialAudioProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl SpatialAudioProcessor {
    pub fn new() -> Self {
        Self {
            state: Arc::new(SharedSpatialState {
                enabled: AtomicBool::new(false),
                mode: AtomicU8::new(SpatialMode::Studio as u8),
            }),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.state.enabled.store(enabled, Ordering::Release);
    }

    pub fn is_enabled(&self) -> bool {
        self.state.enabled.load(Ordering::Acquire)
    }

    pub fn set_mode(&self, mode: SpatialMode) {
        self.state.mode.store(mode as u8, Ordering::Release);
    }

    pub fn mode(&self) -> SpatialMode {
        SpatialMode::from_u8(self.state.mode.load(Ordering::Acquire))
    }

    #[inline]
    pub fn active_mode(&self) -> SpatialMode {
        if !self.is_enabled() {
            SpatialMode::Off
        } else {
            self.mode()
        }
    }
}

pub struct SpatialSource<I> {
    inner: I,
    processor: SpatialAudioProcessor,
    core: SpatialDspCore,
    channels: u16,
    channel_idx: u16,
}

impl<I> SpatialSource<I>
where
    I: rodio::Source<Item = f32>,
{
    pub fn new(inner: I, processor: SpatialAudioProcessor) -> Self {
        let channels = inner.channels();
        let sample_rate = inner.sample_rate();
        Self {
            inner,
            processor,
            core: SpatialDspCore::new(sample_rate),
            channels,
            channel_idx: 0,
        }
    }
}

impl<I> Iterator for SpatialSource<I>
where
    I: rodio::Source<Item = f32>,
{
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next()?;
        let is_right = self.channel_idx == 1;
        self.channel_idx = if self.channels > 0 {
            (self.channel_idx + 1) % self.channels
        } else {
            0
        };

        let mode = self.processor.active_mode();
        if self.channels != 2 || mode == SpatialMode::Off {
            if self.core.was_active {
                self.core.reset();
                self.core.last_mode = SpatialMode::Off;
            }
            return Some(sample);
        }

        Some(self.core.process_sample(sample, is_right, mode))
    }
}

impl<I> rodio::Source for SpatialSource<I>
where
    I: rodio::Source<Item = f32>,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}