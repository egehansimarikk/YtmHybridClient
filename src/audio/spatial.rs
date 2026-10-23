//! Spatial Audio / 3D Soundstage Core Processing Module.
//!
//! Provides real-time DSP pipelines for 2-channel stereo PCM audio streams:
//!
//! 1. `DolbyAtmosPassthrough`: Configures WASAPI stream flags (`AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM`
//!    and spatial object metadata) so Windows routes audio through system-level Dolby Atmos for
//!    Headphones, DTS Headphone:X, or Windows Sonic.
//! 2. `CustomHRTF`: Low-latency binaural HRTF (Head-Related Transfer Function) spatializer algorithm
//!    utilizing spherical head diffraction filtering, pinna notch cues, and sub-sample fractional
//!    ITD (Interaural Time Difference) delay lines to widen the stereo soundstage outside the head.
//! 3. `BinauralCrossfeed`: Frequency-dependent Bauer / Linkwitz crossfeed filter simulating physical
//!    loudspeakers on IEMs (In-Ear Monitors) and headphones to eliminate extreme channel isolation fatigue.
//!
//! Processing guarantees:
//! - Zero algorithmic buffer latency (sample-by-sample and block-level direct processing).
//! - Click-free, pop-free transitions via equal-power crossfading (`CrossFader`) on dynamic mode switches.
//! - Zero dynamic heap allocation in the audio rendering hot loop.

use std::f32::consts::PI;
use std::time::Duration;

// ==============================================================================
// WASAPI Stream Flags & Constants
// ==============================================================================

/// WASAPI stream flag instructing the Windows Audio Engine to automatically convert
/// the client audio format to the engine mix format and route to spatial processing objects (SAPO).
pub const AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM: u32 = 0x8000_0000;

/// WASAPI stream flag specifying default quality sample rate conversion.
pub const AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY: u32 = 0x0800_0000;

/// WASAPI stream flag for event-driven audio buffer completion callbacks.
pub const AUDCLNT_STREAMFLAGS_EVENTCALLBACK: u32 = 0x0004_0000;

/// Default transition crossfade duration (35 ms ~ 1680 samples at 48 kHz).
pub const DEFAULT_RAMP_DURATION: Duration = Duration::from_millis(35);

/// Maximum delay buffer capacity in samples (~10.6 ms at 192 kHz, ~42 ms at 48 kHz).
const MAX_DELAY_CAPACITY: usize = 2048;

// ==============================================================================
// Error Handling
// ==============================================================================

/// Audio error type returned across the spatial audio pipeline.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioError {
    /// Audio output device is unavailable or was disconnected.
    DeviceUnavailable(String),
    /// Audio format (channel count or sample rate) is unsupported.
    UnsupportedFormat { channels: u16, sample_rate: u32 },
    /// Initialization of WASAPI client or spatial audio object stream failed.
    WasapiInitFailed(String),
    /// System-level spatial audio is disabled or unsupported by the active endpoint.
    SpatialAudioUnsupported(String),
    /// Buffer underrun occurred during real-time rendering.
    BufferUnderflow,
    /// Channel count does not match expected stereo pair.
    ChannelMismatch { expected: usize, found: usize },
    /// An invalid DSP parameter was supplied.
    InvalidParameter(String),
    /// Synchronization lock acquisition failure.
    LockError(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeviceUnavailable(msg) => write!(f, "Audio device unavailable: {msg}"),
            Self::UnsupportedFormat { channels, sample_rate } => {
                write!(f, "Unsupported format: {channels} channels, {sample_rate} Hz")
            }
            Self::WasapiInitFailed(msg) => write!(f, "WASAPI initialization failed: {msg}"),
            Self::SpatialAudioUnsupported(msg) => write!(f, "Spatial audio unsupported: {msg}"),
            Self::BufferUnderflow => write!(f, "Audio buffer underflow"),
            Self::ChannelMismatch { expected, found } => {
                write!(f, "Channel mismatch: expected {expected}, found {found}")
            }
            Self::InvalidParameter(msg) => write!(f, "Invalid parameter: {msg}"),
            Self::LockError(msg) => write!(f, "Lock contention: {msg}"),
        }
    }
}

impl std::error::Error for AudioError {}

// ==============================================================================
// Spatial Audio Modes
// ==============================================================================

/// Supported spatial audio modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum SpatialMode {
    /// System-level Dolby Atmos for Headphones / DTS Headphone:X / Windows Sonic via WASAPI flags.
    #[default]
    DolbyAtmosPassthrough,
    /// Low-latency binaural HRTF spatializer widening the stereo soundstage outside the listener's head.
    CustomHRTF,
    /// Bauer / Linkwitz frequency-dependent crossfeed filter for natural speaker simulation on headphones.
    BinauralCrossfeed,
}

impl SpatialMode {
    /// List of all modes for UI selector population.
    pub const ALL: [Self; 3] = [
        Self::DolbyAtmosPassthrough,
        Self::CustomHRTF,
        Self::BinauralCrossfeed,
    ];

    /// Canonical identifier string.
    pub fn id(self) -> &'static str {
        match self {
            Self::DolbyAtmosPassthrough => "dolby_atmos_passthrough",
            Self::CustomHRTF => "custom_hrtf",
            Self::BinauralCrossfeed => "binaural_crossfeed",
        }
    }

    /// User-friendly display label.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::DolbyAtmosPassthrough => "Dolby Atmos Passthrough",
            Self::CustomHRTF => "Custom HRTF (3D Soundstage)",
            Self::BinauralCrossfeed => "Binaural Crossfeed (Speaker Sim)",
        }
    }

    /// Explanatory description.
    pub fn description(self) -> &'static str {
        match self {
            Self::DolbyAtmosPassthrough => {
                "Enables Windows Spatial Audio routing (Dolby Atmos for Headphones / DTS Headphone:X)."
            }
            Self::CustomHRTF => {
                "Binaural spherical head model and pinna notch cues projecting sound outside the skull."
            }
            Self::BinauralCrossfeed => {
                "Bauer/Linkwitz frequency-dependent acoustic crossfeed simulating natural stereo speakers."
            }
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "dolby_atmos_passthrough" => Some(Self::DolbyAtmosPassthrough),
            "custom_hrtf" => Some(Self::CustomHRTF),
            "binaural_crossfeed" => Some(Self::BinauralCrossfeed),
            _ => None,
        }
    }
}

// ==============================================================================
// Biquad IIR Filter (Direct Form II Transposed)
// ==============================================================================

/// Numerically stable Direct Form II Transposed biquad filter.
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    s1: f32,
    s2: f32,
}

impl Biquad {
    pub fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    /// 1st-order low-pass filter (for Bauer acoustic crossfeed).
    pub fn lowpass_1st_order(cutoff_hz: f32, sample_rate: f32) -> Self {
        let omega = 2.0 * PI * cutoff_hz.clamp(10.0, sample_rate * 0.49) / sample_rate;
        let alpha = omega / (1.0 + omega);

        Self {
            b0: alpha,
            b1: alpha,
            b2: 0.0,
            a1: -(1.0 - alpha),
            a2: 0.0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    /// 2nd-order Butterworth low-pass filter.
    pub fn lowpass(cutoff_hz: f32, sample_rate: f32) -> Self {
        let omega = 2.0 * PI * cutoff_hz.clamp(10.0, sample_rate * 0.49) / sample_rate;
        let sn = omega.sin();
        let cs = omega.cos();
        let alpha = sn / (2.0 * std::f32::consts::FRAC_1_SQRT_2);

        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 - cs) * 0.5) / a0,
            b1: (1.0 - cs) / a0,
            b2: ((1.0 - cs) * 0.5) / a0,
            a1: (-2.0 * cs) / a0,
            a2: (1.0 - alpha) / a0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    /// Low-shelf filter for frequency-dependent crossfeed bass balancing.
    pub fn lowshelf(frequency: f32, gain_db: f32, q: f32, sample_rate: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let omega = 2.0 * PI * frequency.clamp(10.0, sample_rate * 0.49) / sample_rate;
        let sn = omega.sin();
        let cs = omega.cos();
        let alpha = sn / (2.0 * q.max(0.1));
        let beta = 2.0 * a.sqrt() * alpha;

        let a0 = (a + 1.0) + (a - 1.0) * cs + beta;
        Self {
            b0: (a * ((a + 1.0) - (a - 1.0) * cs + beta)) / a0,
            b1: (2.0 * a * ((a - 1.0) - (a + 1.0) * cs)) / a0,
            b2: (a * ((a + 1.0) - (a - 1.0) * cs - beta)) / a0,
            a1: (-2.0 * ((a - 1.0) + (a + 1.0) * cs)) / a0,
            a2: ((a + 1.0) + (a - 1.0) * cs - beta) / a0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    /// High-shelf filter for contralateral head shadow simulation.
    pub fn highshelf(frequency: f32, gain_db: f32, q: f32, sample_rate: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let omega = 2.0 * PI * frequency.clamp(10.0, sample_rate * 0.49) / sample_rate;
        let sn = omega.sin();
        let cs = omega.cos();
        let alpha = sn / (2.0 * q.max(0.1));
        let beta = 2.0 * a.sqrt() * alpha;

        let a0 = (a + 1.0) - (a - 1.0) * cs + beta;
        Self {
            b0: (a * ((a + 1.0) + (a - 1.0) * cs + beta)) / a0,
            b1: (-2.0 * a * ((a - 1.0) + (a + 1.0) * cs)) / a0,
            b2: (a * ((a + 1.0) + (a - 1.0) * cs - beta)) / a0,
            a1: (2.0 * ((a - 1.0) - (a + 1.0) * cs)) / a0,
            a2: ((a + 1.0) - (a - 1.0) * cs - beta) / a0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    /// Peaking / notch filter (for pinna concha spectral localization cues).
    pub fn peaking(frequency: f32, gain_db: f32, q: f32, sample_rate: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let omega = 2.0 * PI * frequency.clamp(10.0, sample_rate * 0.49) / sample_rate;
        let sn = omega.sin();
        let cs = omega.cos();
        let alpha = sn / (2.0 * q.max(0.1));

        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cs) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cs) / a0,
            a2: (1.0 - alpha / a) / a0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    #[inline(always)]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
}

// ==============================================================================
// Fractional Delay Line
// ==============================================================================

/// Circular delay line with sub-sample linear interpolation.
#[derive(Clone, Debug)]
pub struct FractionalDelay {
    buffer: Vec<f32>,
    write_pos: usize,
    capacity: usize,
}

impl FractionalDelay {
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(64);
        Self {
            buffer: vec![0.0; cap],
            write_pos: 0,
            capacity: cap,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, sample: f32) {
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % self.capacity;
    }

    #[inline(always)]
    pub fn read_delay(&self, delay_samples: f32) -> f32 {
        let delay_clamped = delay_samples.clamp(0.0, (self.capacity - 2) as f32);
        let d_floor = delay_clamped.floor() as usize;
        let frac = delay_clamped - d_floor as f32;

        let idx0 = (self.write_pos + self.capacity - 1 - d_floor) % self.capacity;
        let idx1 = (self.write_pos + self.capacity - 2 - d_floor) % self.capacity;

        let s0 = self.buffer[idx0];
        let s1 = self.buffer[idx1];
        s0 + frac * (s1 - s0)
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
    }
}

// ==============================================================================
// Mode Processors
// ==============================================================================

/// Dolby Atmos / DTS Passthrough Engine.
#[derive(Clone, Debug)]
pub struct DolbyAtmosPassthroughEngine {
    headroom_gain: f32,
    wasapi_flags: u32,
}

impl DolbyAtmosPassthroughEngine {
    pub fn new() -> Self {
        // -1.5 dB headroom protects against inter-sample peaks during Dolby Atmos binaural rendering
        Self {
            headroom_gain: 10.0f32.powf(-1.5 / 20.0),
            wasapi_flags: AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
        }
    }

    pub fn wasapi_stream_flags(&self) -> u32 {
        self.wasapi_flags
    }

    #[inline(always)]
    pub fn process_frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        (left * self.headroom_gain, right * self.headroom_gain)
    }
}

impl Default for DolbyAtmosPassthroughEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Custom HRTF 3D Soundstage Engine.
#[derive(Clone, Debug)]
pub struct CustomHrtfEngine {
    sample_rate: f32,
    soundstage_width: f32,
    contra_filter_l: Biquad,
    contra_filter_r: Biquad,
    pinna_filter_l: Biquad,
    pinna_filter_r: Biquad,
    delay_l: FractionalDelay,
    delay_r: FractionalDelay,
    itd_samples: f32,
}

impl CustomHrtfEngine {
    pub fn new(sample_rate: u32, soundstage_width: f32) -> Self {
        let sr = sample_rate as f32;
        let mut engine = Self {
            sample_rate: sr,
            soundstage_width: soundstage_width.clamp(0.5, 2.0),
            contra_filter_l: Biquad::identity(),
            contra_filter_r: Biquad::identity(),
            pinna_filter_l: Biquad::identity(),
            pinna_filter_r: Biquad::identity(),
            delay_l: FractionalDelay::new(MAX_DELAY_CAPACITY),
            delay_r: FractionalDelay::new(MAX_DELAY_CAPACITY),
            itd_samples: 0.0,
        };
        engine.recompute();
        engine
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        if self.sample_rate != sample_rate as f32 {
            self.sample_rate = sample_rate as f32;
            self.recompute();
        }
    }

    pub fn set_width(&mut self, width: f32) {
        self.soundstage_width = width.clamp(0.5, 2.0);
        self.recompute();
    }

    fn recompute(&mut self) {
        let sr = self.sample_rate;
        // Woodworth formula for ~35 deg azimuth: ITD ~ 0.28 ms
        let itd_sec = 0.00028 * self.soundstage_width;
        self.itd_samples = (itd_sec * sr).clamp(1.0, (MAX_DELAY_CAPACITY - 4) as f32);

        // Contralateral head shadow: -8.5 dB high shelf at 1800 Hz
        let contra = Biquad::highshelf(1800.0, -8.5, 0.707, sr);
        self.contra_filter_l = contra;
        self.contra_filter_r = contra;

        // Ipsilateral pinna notch cue: -6.0 dB notch at 6500 Hz, Q = 2.5
        let pinna = Biquad::peaking(6500.0, -6.0, 2.5, sr);
        self.pinna_filter_l = pinna;
        self.pinna_filter_r = pinna;
    }

    #[inline(always)]
    pub fn process_frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        self.delay_l.push(left);
        self.delay_r.push(right);

        let direct_l = self.pinna_filter_l.process(left);
        let direct_r = self.pinna_filter_r.process(right);

        let del_r = self.delay_r.read_delay(self.itd_samples);
        let del_l = self.delay_l.read_delay(self.itd_samples);

        let cross_to_l = self.contra_filter_l.process(del_r);
        let cross_to_r = self.contra_filter_r.process(del_l);

        let bin_l = direct_l + 0.55 * cross_to_l;
        let bin_r = direct_r + 0.55 * cross_to_r;

        // Mid-side spatial widening
        let mid = (bin_l + bin_r) * 0.5;
        let side = (bin_l - bin_r) * 0.5;
        let width = self.soundstage_width.clamp(0.8, 1.6);
        let out_l = (mid + side * width) * 0.82;
        let out_r = (mid - side * width) * 0.82;

        (out_l, out_r)
    }

    pub fn reset(&mut self) {
        self.contra_filter_l.reset();
        self.contra_filter_r.reset();
        self.pinna_filter_l.reset();
        self.pinna_filter_r.reset();
        self.delay_l.reset();
        self.delay_r.reset();
    }
}

/// Binaural Crossfeed Engine (Bauer / Linkwitz).
#[derive(Clone, Debug)]
pub struct BinauralCrossfeedEngine {
    sample_rate: f32,
    crossfeed_amount: f32,
    cross_filter_l: Biquad,
    cross_filter_r: Biquad,
    direct_filter_l: Biquad,
    direct_filter_r: Biquad,
    delay_l: FractionalDelay,
    delay_r: FractionalDelay,
    delay_samples: f32,
}

impl BinauralCrossfeedEngine {
    pub fn new(sample_rate: u32, crossfeed_amount: f32) -> Self {
        let sr = sample_rate as f32;
        let mut engine = Self {
            sample_rate: sr,
            crossfeed_amount: crossfeed_amount.clamp(0.0, 1.0),
            cross_filter_l: Biquad::identity(),
            cross_filter_r: Biquad::identity(),
            direct_filter_l: Biquad::identity(),
            direct_filter_r: Biquad::identity(),
            delay_l: FractionalDelay::new(MAX_DELAY_CAPACITY),
            delay_r: FractionalDelay::new(MAX_DELAY_CAPACITY),
            delay_samples: 0.0,
        };
        engine.recompute();
        engine
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        if self.sample_rate != sample_rate as f32 {
            self.sample_rate = sample_rate as f32;
            self.recompute();
        }
    }

    pub fn set_amount(&mut self, amount: f32) {
        self.crossfeed_amount = amount.clamp(0.0, 1.0);
        self.recompute();
    }

    fn recompute(&mut self) {
        let sr = self.sample_rate;
        let delay_sec = 0.00028;
        self.delay_samples = (delay_sec * sr).clamp(1.0, (MAX_DELAY_CAPACITY - 4) as f32);

        let lp = Biquad::lowpass_1st_order(700.0, sr);
        self.cross_filter_l = lp;
        self.cross_filter_r = lp;

        let shelf = Biquad::lowshelf(700.0, 1.5, 0.707, sr);
        self.direct_filter_l = shelf;
        self.direct_filter_r = shelf;
    }

    #[inline(always)]
    pub fn process_frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        self.delay_l.push(left);
        self.delay_r.push(right);

        let dir_l = self.direct_filter_l.process(left);
        let dir_r = self.direct_filter_r.process(right);

        let del_r = self.delay_r.read_delay(self.delay_samples);
        let del_l = self.delay_l.read_delay(self.delay_samples);

        let cross_l = self.cross_filter_l.process(del_r);
        let cross_r = self.cross_filter_r.process(del_l);

        let cross_gain = 0.58 * self.crossfeed_amount;
        let norm = 1.0 / (1.0 + cross_gain * 0.45);

        let out_l = (dir_l + cross_l * cross_gain) * norm;
        let out_r = (dir_r + cross_r * cross_gain) * norm;

        (out_l, out_r)
    }

    pub fn reset(&mut self) {
        self.cross_filter_l.reset();
        self.cross_filter_r.reset();
        self.direct_filter_l.reset();
        self.direct_filter_r.reset();
        self.delay_l.reset();
        self.delay_r.reset();
    }
}

// ==============================================================================
// Equal-Power Crossfader (Click & Pop Prevention)
// ==============================================================================

/// Equal-power sinusoidal crossfader ensuring continuous, click-free audio transitions.
#[derive(Clone, Debug)]
pub struct CrossFader {
    fade_frames: u32,
    frames_remaining: u32,
}

impl CrossFader {
    pub fn new(ramp: Duration, sample_rate: u32) -> Self {
        let frames = (ramp.as_secs_f64() * sample_rate as f64).round().max(1.0) as u32;
        Self {
            fade_frames: frames,
            frames_remaining: 0,
        }
    }

    pub fn set_sample_rate(&mut self, ramp: Duration, sample_rate: u32) {
        self.fade_frames = (ramp.as_secs_f64() * sample_rate as f64).round().max(1.0) as u32;
    }

    pub fn trigger(&mut self) {
        self.frames_remaining = self.fade_frames;
    }

    pub fn is_fading(&self) -> bool {
        self.frames_remaining > 0
    }

    #[inline(always)]
    pub fn next_gains(&mut self) -> (f32, f32) {
        if self.frames_remaining == 0 {
            return (0.0, 1.0);
        }
        let progress = 1.0 - (self.frames_remaining as f32 / self.fade_frames as f32);
        self.frames_remaining -= 1;
        let angle = progress * (PI * 0.5);
        (angle.cos(), angle.sin())
    }
}
