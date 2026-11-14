//! A ten-band Parametric & Graphic Equalizer applied to the interleaved PCM stream
//! just before the volume ramp.
//!
//! `Equalizer` is the lock-free atomic handle held by UI and settings, while `Equalized`
//! is the `rodio::Source` wrapper running on the audio thread. Supports both classic
//! 10-band octave graphic presets and full Parametric EQ / AutoEQ `.txt` profiles
//! (`Preamp`, `PK`, `LSC`, `HSC`, per-band `Fc`, `Gain`, and `Q`).

use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rodio::source::SeekError;
use rodio::Source;

/// How many bands the equalizer has (10 bands).
pub const BANDS: usize = 10;

/// Default ISO octave centre frequencies in hertz for graphic EQ mode.
pub const FREQUENCIES: [f32; BANDS] = [
    31.25, 62.5, 125., 250., 500., 1_000., 2_000., 4_000., 8_000., 16_000.,
];

/// The lowest gain a graphic EQ slider accepts, in decibels.
pub const MIN_GAIN: f32 = -12.;
/// The highest gain a graphic EQ slider accepts, in decibels.
pub const MAX_GAIN: f32 = 12.;

/// Extended gain bounds for Parametric EQ / AutoEQ profiles, in decibels.
pub const PARAMETRIC_MIN_GAIN: f32 = -24.;
pub const PARAMETRIC_MAX_GAIN: f32 = 24.;

/// Preamp gain bounds, in decibels.
pub const MIN_PREAMP: f32 = -24.;
pub const MAX_PREAMP: f32 = 12.;

/// Per-band gains, in decibels, from the lowest band to the highest.
pub type Gains = [f32; BANDS];

/// Default one-octave bandwidth Q factor (`sqrt(2)`).
pub const DEFAULT_Q: f32 = std::f32::consts::SQRT_2;

/// How long parameter changes take to settle, preventing zipper noise / clicks.
const RAMP: Duration = Duration::from_millis(40);
/// Bands above 95% of Nyquist are bypassed to avoid frequency warping/aliasing.
const NYQUIST_MARGIN: f32 = 0.95;

// ==============================================================================
// Filter Types & Parametric Band Definition
// ==============================================================================

/// Biquad filter topology supported by the Parametric EQ and AutoEQ parser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum FilterType {
    /// Bell / Peaking filter (`PK` in AutoEQ / EqualizerAPO).
    #[default]
    Peaking = 0,
    /// Low-shelf filter (`LSC` / `LS` in AutoEQ / EqualizerAPO).
    LowShelf = 1,
    /// High-shelf filter (`HSC` / `HS` in AutoEQ / EqualizerAPO).
    HighShelf = 2,
}

impl FilterType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::LowShelf,
            2 => Self::HighShelf,
            _ => Self::Peaking,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Peaking => "PK",
            Self::LowShelf => "LSC",
            Self::HighShelf => "HSC",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code.trim().to_ascii_uppercase().as_str() {
            "PK" | "PEAKING" => Some(Self::Peaking),
            "LSC" | "LS" | "LOWSHELF" | "LOW_SHELF" => Some(Self::LowShelf),
            "HSC" | "HS" | "HIGHSHELF" | "HIGH_SHELF" => Some(Self::HighShelf),
            _ => None,
        }
    }
}

/// Complete configuration for a single parametric EQ band.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParametricBand {
    pub enabled: bool,
    pub filter_type: FilterType,
    pub frequency: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl ParametricBand {
    pub fn graphic(frequency: f32, gain_db: f32) -> Self {
        Self {
            enabled: true,
            filter_type: FilterType::Peaking,
            frequency: frequency.clamp(10.0, 22_000.0),
            gain_db: gain_db.clamp(MIN_GAIN, MAX_GAIN),
            q: DEFAULT_Q,
        }
    }

    pub fn sanitized(mut self) -> Self {
        self.frequency = if self.frequency.is_finite() {
            self.frequency.clamp(10.0, 22_000.0)
        } else {
            1_000.0
        };
        self.gain_db = if self.gain_db.is_finite() {
            self.gain_db.clamp(PARAMETRIC_MIN_GAIN, PARAMETRIC_MAX_GAIN)
        } else {
            0.0
        };
        self.q = if self.q.is_finite() {
            self.q.clamp(0.1, 20.0)
        } else {
            DEFAULT_Q
        };
        self
    }
}

impl Default for ParametricBand {
    fn default() -> Self {
        Self {
            enabled: true,
            filter_type: FilterType::Peaking,
            frequency: 1_000.0,
            gain_db: 0.0,
            q: DEFAULT_Q,
        }
    }
}

/// Parsed AutoEQ / EqualizerAPO parametric profile.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AutoEqProfile {
    pub name: String,
    pub preamp_db: f32,
    pub bands: [ParametricBand; BANDS],
}

impl Default for AutoEqProfile {
    fn default() -> Self {
        Self {
            name: "Flat".to_owned(),
            preamp_db: 0.0,
            bands: std::array::from_fn(|i| ParametricBand::graphic(FREQUENCIES[i], 0.0)),
        }
    }
}

impl AutoEqProfile {
    /// Parses a standard AutoEQ / EqualizerAPO `.txt` file content.
    ///
    /// Example supported lines:
    /// ```text
    /// Preamp: -6.4 dB
    /// Filter 1: ON PK Fc 21 Hz Gain 4.8 dB Q 0.65
    /// Filter 2: ON LSC Fc 105 Hz Gain 2.1 dB Q 0.70
    /// Filter 3: ON HSC Fc 10000 Hz Gain -3.2 dB Q 0.71
    /// ```
    pub fn from_autoeq_txt(name: impl Into<String>, text: &str) -> Result<Self, String> {
        let mut preamp_db = 0.0f32;
        let mut parsed_bands: Vec<ParametricBand> = Vec::with_capacity(BANDS);

        for raw_line in text.lines() {
            let line = raw_line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }

            if let Some(rest) = line.strip_prefix("Preamp:").or_else(|| line.strip_prefix("preamp:"))
            {
                let val_str = rest
                    .trim()
                    .trim_end_matches("dB")
                    .trim_end_matches("db")
                    .trim();
                if let Ok(val) = val_str.parse::<f32>() {
                    preamp_db = val.clamp(MIN_PREAMP, MAX_PREAMP);
                }
                continue;
            }

            if line.starts_with("Filter") || line.starts_with("filter") {
                if let Some(band) = parse_filter_line(line) {
                    if band.enabled && parsed_bands.len() < BANDS {
                        parsed_bands.push(band.sanitized());
                    }
                }
            }
        }

        if parsed_bands.is_empty() {
            return Err("No valid enabled EQ filters found in AutoEQ file.".to_owned());
        }

        let mut bands = std::array::from_fn(|i| ParametricBand::graphic(FREQUENCIES[i], 0.0));
        for (dst, src) in bands.iter_mut().zip(parsed_bands.into_iter()) {
            *dst = src;
        }

        Ok(Self {
            name: name.into(),
            preamp_db,
            bands,
        })
    }

    /// Formats this profile into standard EqualizerAPO / AutoEQ `.txt` syntax.
    pub fn to_autoeq_txt(&self) -> String {
        let mut out = format!("Preamp: {:.1} dB\n", self.preamp_db);
        for (i, band) in self.bands.iter().enumerate() {
            let state = if band.enabled { "ON" } else { "OFF" };
            out.push_str(&format!(
                "Filter {}: {} {} Fc {:.1} Hz Gain {:.1} dB Q {:.2}\n",
                i + 1,
                state,
                band.filter_type.code(),
                band.frequency,
                band.gain_db,
                band.q
            ));
        }
        out
    }
}

fn parse_filter_line(line: &str) -> Option<ParametricBand> {
    // Expected: "Filter N: ON PK Fc 21 Hz Gain 4.8 dB Q 0.65"
    let (_, rhs) = line.split_once(':')?;
    let tokens: Vec<&str> = rhs.split_whitespace().collect();
    if tokens.len() < 2 {
        return None;
    }

    let enabled = match tokens[0].to_ascii_uppercase().as_str() {
        "ON" => true,
        "OFF" => false,
        _ => return None,
    };

    let filter_type = FilterType::from_code(tokens[1])?;
    let mut frequency = 1_000.0f32;
    let mut gain_db = 0.0f32;
    let mut q = DEFAULT_Q;

    let mut idx = 2;
    while idx < tokens.len() {
        match tokens[idx].to_ascii_uppercase().as_str() {
            "FC" if idx + 1 < tokens.len() => {
                frequency = tokens[idx + 1]
                    .trim_end_matches("Hz")
                    .trim_end_matches("hz")
                    .parse::<f32>()
                    .ok()?;
                idx += 2;
            }
            "GAIN" if idx + 1 < tokens.len() => {
                gain_db = tokens[idx + 1]
                    .trim_end_matches("dB")
                    .trim_end_matches("db")
                    .parse::<f32>()
                    .ok()?;
                idx += 2;
            }
            "Q" if idx + 1 < tokens.len() => {
                q = tokens[idx + 1].parse::<f32>().ok()?;
                idx += 2;
            }
            _ => {
                idx += 1;
            }
        }
    }

    Some(ParametricBand {
        enabled,
        filter_type,
        frequency,
        gain_db,
        q,
    })
}

// ==============================================================================
// Graphic EQ Presets (Backward-Compatible)
// ==============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Flat,
    BassBoost,
    BassReducer,
    TrebleBoost,
    Vocal,
    Rock,
    Pop,
    Jazz,
    Classical,
    Electronic,
    Acoustic,
    Loudness,
}

impl Preset {
    pub const ALL: [Self; 12] = [
        Self::Flat,
        Self::BassBoost,
        Self::BassReducer,
        Self::TrebleBoost,
        Self::Vocal,
        Self::Rock,
        Self::Pop,
        Self::Jazz,
        Self::Classical,
        Self::Electronic,
        Self::Acoustic,
        Self::Loudness,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::BassBoost => "bass-boost",
            Self::BassReducer => "bass-reducer",
            Self::TrebleBoost => "treble-boost",
            Self::Vocal => "vocal",
            Self::Rock => "rock",
            Self::Pop => "pop",
            Self::Jazz => "jazz",
            Self::Classical => "classical",
            Self::Electronic => "electronic",
            Self::Acoustic => "acoustic",
            Self::Loudness => "loudness",
        }
    }

    pub fn gains(self) -> Gains {
        match self {
            Self::Flat => [0.; BANDS],
            Self::BassBoost => [6., 5., 4., 2.5, 1., 0., 0., 0., 0., 0.],
            Self::BassReducer => [-6., -5., -4., -2.5, -1., 0., 0., 0., 0., 0.],
            Self::TrebleBoost => [0., 0., 0., 0., 0., 1., 2.5, 4., 5., 6.],
            Self::Vocal => [-2., -3., -3., 1., 4., 4., 3., 1.5, 0., -1.5],
            Self::Rock => [5., 4., 3., 1., -0.5, -1., 0.5, 2.5, 3.5, 4.5],
            Self::Pop => [-1.5, -1., 0., 2., 4., 4., 2., 0., -1., -1.5],
            Self::Jazz => [4., 3., 1.5, 2., -1.5, -1.5, 0., 1.5, 3., 4.],
            Self::Classical => [4.5, 3.5, 3., 2.5, -1.5, -1.5, 0., 2.5, 3.5, 4.],
            Self::Electronic => [4.5, 4., 1., 0., -2., 2., 1., 1.5, 4., 5.],
            Self::Acoustic => [5., 4.5, 3.5, 1., 2., 1.5, 3.5, 4., 3.5, 2.],
            Self::Loudness => [6., 4., 0., 0., -2., 0., -1., -5., 5., 1.],
        }
    }

    pub fn matching(gains: &Gains) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| {
            preset
                .gains()
                .iter()
                .zip(gains)
                .all(|(want, have)| (want - have).abs() < 0.05)
        })
    }
}

/// Clamps every band into the graphic EQ accepted range and turns non-finite numbers flat.
pub fn clamped(gains: &Gains) -> Gains {
    gains.map(|gain| match gain.is_finite() {
        true => gain.clamp(MIN_GAIN, MAX_GAIN),
        false => 0.,
    })
}

// ==============================================================================
// Lock-Free Shared State
// ==============================================================================

struct AtomicBand {
    enabled: AtomicBool,
    filter_type: AtomicU8,
    frequency: AtomicU32,
    gain_db: AtomicU32,
    q: AtomicU32,
}

impl AtomicBand {
    fn new(band: ParametricBand) -> Self {
        let band = band.sanitized();
        Self {
            enabled: AtomicBool::new(band.enabled),
            filter_type: AtomicU8::new(band.filter_type as u8),
            frequency: AtomicU32::new(band.frequency.to_bits()),
            gain_db: AtomicU32::new(band.gain_db.to_bits()),
            q: AtomicU32::new(band.q.to_bits()),
        }
    }

    fn load(&self) -> ParametricBand {
        ParametricBand {
            enabled: self.enabled.load(Ordering::Acquire),
            filter_type: FilterType::from_u8(self.filter_type.load(Ordering::Acquire)),
            frequency: f32::from_bits(self.frequency.load(Ordering::Acquire)),
            gain_db: f32::from_bits(self.gain_db.load(Ordering::Acquire)),
            q: f32::from_bits(self.q.load(Ordering::Acquire)),
        }
    }

    fn store(&self, band: ParametricBand) {
        let band = band.sanitized();
        self.enabled.store(band.enabled, Ordering::Release);
        self.filter_type
            .store(band.filter_type as u8, Ordering::Release);
        self.frequency
            .store(band.frequency.to_bits(), Ordering::Release);
        self.gain_db
            .store(band.gain_db.to_bits(), Ordering::Release);
        self.q.store(band.q.to_bits(), Ordering::Release);
    }
}

struct Shared {
    enabled: AtomicBool,
    parametric_mode: AtomicBool,
    preamp_db: AtomicU32,
    bands: [AtomicBand; BANDS],
    generation: AtomicU32,
}

#[derive(Clone, Copy)]
struct EffectiveParams {
    preamp_db: f32,
    bands: [ParametricBand; BANDS],
}

impl EffectiveParams {
    fn flat() -> Self {
        Self {
            preamp_db: 0.0,
            bands: std::array::from_fn(|i| ParametricBand::graphic(FREQUENCIES[i], 0.0)),
        }
    }
}

/// Lock-free equalizer handle shared across GPUI views, state, and audio threads.
#[derive(Clone)]
pub struct Equalizer(Arc<Shared>);

impl Equalizer {
    pub fn new(enabled: bool, gains: &Gains) -> Self {
        let gains = clamped(gains);
        Self(Arc::new(Shared {
            enabled: AtomicBool::new(enabled),
            parametric_mode: AtomicBool::new(false),
            preamp_db: AtomicU32::new(0.0f32.to_bits()),
            bands: std::array::from_fn(|i| {
                AtomicBand::new(ParametricBand::graphic(FREQUENCIES[i], gains[i]))
            }),
            generation: AtomicU32::new(0),
        }))
    }

    pub fn enabled(&self) -> bool {
        self.0.enabled.load(Ordering::Acquire)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.store(enabled, Ordering::Release);
        self.0.generation.fetch_add(1, Ordering::Release);
    }

    pub fn is_parametric(&self) -> bool {
        self.0.parametric_mode.load(Ordering::Acquire)
    }

    pub fn preamp_db(&self) -> f32 {
        f32::from_bits(self.0.preamp_db.load(Ordering::Acquire))
    }

    pub fn set_preamp_db(&self, preamp_db: f32) {
        let clamped_preamp = if preamp_db.is_finite() {
            preamp_db.clamp(MIN_PREAMP, MAX_PREAMP)
        } else {
            0.0
        };
        self.0
            .preamp_db
            .store(clamped_preamp.to_bits(), Ordering::Release);
        self.0.generation.fetch_add(1, Ordering::Release);
    }

    /// Returns the 10 band gains in decibels (compatible with graphic EQ sliders).
    pub fn gains(&self) -> Gains {
        std::array::from_fn(|band| f32::from_bits(self.0.bands[band].gain_db.load(Ordering::Acquire)))
    }

    /// Sets 10-band graphic EQ gains (resets frequencies to ISO octaves and Q to sqrt(2)).
    pub fn set_gains(&self, gains: &Gains) {
        self.0.parametric_mode.store(false, Ordering::Release);
        self.0
            .preamp_db
            .store(0.0f32.to_bits(), Ordering::Release);
        for (i, (slot, gain)) in self.0.bands.iter().zip(clamped(gains)).enumerate() {
            slot.store(ParametricBand::graphic(FREQUENCIES[i], gain));
        }
        self.0.generation.fetch_add(1, Ordering::Release);
    }

    /// Returns all 10 parametric band configurations.
    pub fn bands(&self) -> [ParametricBand; BANDS] {
        std::array::from_fn(|i| self.0.bands[i].load())
    }

    /// Updates a single parametric band (`0..10`) and switches to parametric mode.
    pub fn set_band(&self, index: usize, band: ParametricBand) {
        if let Some(slot) = self.0.bands.get(index) {
            self.0.parametric_mode.store(true, Ordering::Release);
            slot.store(band);
            self.0.generation.fetch_add(1, Ordering::Release);
        }
    }

    /// Applies an entire parsed `AutoEqProfile` atomically and enables the equalizer.
    pub fn apply_autoeq(&self, profile: &AutoEqProfile) {
        self.0.parametric_mode.store(true, Ordering::Release);
        self.0.preamp_db.store(
            profile
                .preamp_db
                .clamp(MIN_PREAMP, MAX_PREAMP)
                .to_bits(),
            Ordering::Release,
        );
        for (slot, band) in self.0.bands.iter().zip(profile.bands.iter()) {
            slot.store(*band);
        }
        self.0.enabled.store(true, Ordering::Release);
        self.0.generation.fetch_add(1, Ordering::Release);
    }

    /// Exports the current EQ state in AutoEQ / EqualizerAPO `.txt` format.
    pub fn export_autoeq_txt(&self, name: impl Into<String>) -> String {
        let profile = AutoEqProfile {
            name: name.into(),
            preamp_db: self.preamp_db(),
            bands: self.bands(),
        };
        profile.to_autoeq_txt()
    }

    fn effective(&self) -> EffectiveParams {
        if !self.enabled() {
            return EffectiveParams::flat();
        }
        EffectiveParams {
            preamp_db: self.preamp_db(),
            bands: self.bands(),
        }
    }

    fn generation(&self) -> u32 {
        self.0.generation.load(Ordering::Acquire)
    }
}

impl std::fmt::Debug for Equalizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Equalizer")
            .field("enabled", &self.enabled())
            .field("parametric", &self.is_parametric())
            .field("preamp_db", &self.preamp_db())
            .field("gains", &self.gains())
            .finish()
    }
}

impl Default for Equalizer {
    fn default() -> Self {
        Self::new(false, &[0.; BANDS])
    }
}

// ==============================================================================
// RBJ Audio EQ Cookbook Biquad Filter (Transposed Direct Form II)
// ==============================================================================

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    fn from_band(band: ParametricBand, gain_override: f32, freq_override: f32, q_override: f32, rate: f32) -> Self {
        if !band.enabled || gain_override.abs() < 1e-4 || freq_override >= rate * 0.5 * NYQUIST_MARGIN {
            return Self::identity();
        }

        match band.filter_type {
            FilterType::Peaking => Self::peaking(freq_override, gain_override, q_override, rate),
            FilterType::LowShelf => Self::low_shelf(freq_override, gain_override, q_override, rate),
            FilterType::HighShelf => Self::high_shelf(freq_override, gain_override, q_override, rate),
        }
    }

    fn peaking(frequency: f32, gain_db: f32, q: f32, rate: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.);
        let w0 = std::f32::consts::TAU * frequency / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2. * q.max(0.1));

        let a0 = 1. + alpha / a;
        Self {
            b0: (1. + alpha * a) / a0,
            b1: (-2. * cos) / a0,
            b2: (1. - alpha * a) / a0,
            a1: (-2. * cos) / a0,
            a2: (1. - alpha / a) / a0,
        }
    }

    fn low_shelf(frequency: f32, gain_db: f32, q: f32, rate: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.);
        let w0 = std::f32::consts::TAU * frequency / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2. * q.max(0.1));
        let two_sqrt_a_alpha = 2. * a.sqrt() * alpha;

        let a0 = (a + 1.) + (a - 1.) * cos + two_sqrt_a_alpha;
        Self {
            b0: (a * ((a + 1.) - (a - 1.) * cos + two_sqrt_a_alpha)) / a0,
            b1: (2. * a * ((a - 1.) - (a + 1.) * cos)) / a0,
            b2: (a * ((a + 1.) - (a - 1.) * cos - two_sqrt_a_alpha)) / a0,
            a1: (-2. * ((a - 1.) + (a + 1.) * cos)) / a0,
            a2: ((a + 1.) + (a - 1.) * cos - two_sqrt_a_alpha) / a0,
        }
    }

    fn high_shelf(frequency: f32, gain_db: f32, q: f32, rate: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.);
        let w0 = std::f32::consts::TAU * frequency / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2. * q.max(0.1));
        let two_sqrt_a_alpha = 2. * a.sqrt() * alpha;

        let a0 = (a + 1.) - (a - 1.) * cos + two_sqrt_a_alpha;
        Self {
            b0: (a * ((a + 1.) + (a - 1.) * cos + two_sqrt_a_alpha)) / a0,
            b1: (-2. * a * ((a - 1.) + (a + 1.) * cos)) / a0,
            b2: (a * ((a + 1.) + (a - 1.) * cos - two_sqrt_a_alpha)) / a0,
            a1: (2. * ((a - 1.) - (a + 1.) * cos)) / a0,
            a2: ((a + 1.) - (a - 1.) * cos - two_sqrt_a_alpha) / a0,
        }
    }

    fn identity() -> Self {
        Self {
            b0: 1.,
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Delay {
    z1: f32,
    z2: f32,
}

impl Delay {
    #[inline]
    fn run(&mut self, coefficients: &Biquad, x: f32) -> f32 {
        let y = coefficients.b0 * x + self.z1;
        self.z1 = coefficients.b1 * x - coefficients.a1 * y + self.z2;
        self.z2 = coefficients.b2 * x - coefficients.a2 * y;
        y
    }
}

// ==============================================================================
// Rodio Source Wrapper with Click-Free Parameter Ramping
// ==============================================================================

pub struct Equalized<I> {
    input: I,
    equalizer: Equalizer,
    seen: u32,

    current_preamp: f32,
    target_preamp: f32,
    step_preamp: f32,
    preamp_linear: f32,

    current_gains: Gains,
    target_gains: Gains,
    step_gains: Gains,

    current_freqs: [f32; BANDS],
    target_freqs: [f32; BANDS],
    step_freqs: [f32; BANDS],

    current_qs: [f32; BANDS],
    target_qs: [f32; BANDS],
    step_qs: [f32; BANDS],

    active_bands: [ParametricBand; BANDS],

    frames_left: u32,
    ramp_frames: u32,

    coefficients: [Biquad; BANDS],
    delays: Vec<[Delay; BANDS]>,
    active: bool,

    channel: usize,
    channels: usize,
    rate: u32,
}

impl<I: Source> Equalized<I> {
    pub fn new(input: I, equalizer: Equalizer) -> Self {
        let eff = equalizer.effective();
        let seen = equalizer.generation();
        let gains = eff.bands.map(|b| if b.enabled { b.gain_db } else { 0.0 });
        let freqs = eff.bands.map(|b| b.frequency);
        let qs = eff.bands.map(|b| b.q);

        let mut this = Self {
            input,
            equalizer,
            seen,
            current_preamp: eff.preamp_db,
            target_preamp: eff.preamp_db,
            step_preamp: 0.0,
            preamp_linear: 10f32.powf(eff.preamp_db / 20.0),
            current_gains: gains,
            target_gains: gains,
            step_gains: [0.; BANDS],
            current_freqs: freqs,
            target_freqs: freqs,
            step_freqs: [0.; BANDS],
            current_qs: qs,
            target_qs: qs,
            step_qs: [0.; BANDS],
            active_bands: eff.bands,
            frames_left: 0,
            ramp_frames: 1,
            coefficients: [Biquad::identity(); BANDS],
            delays: Vec::new(),
            active: false,
            channel: 0,
            channels: 0,
            rate: 0,
        };
        this.resync();
        this
    }

    fn resync(&mut self) {
        let channels = self.input.channels().get() as usize;
        let rate = self.input.sample_rate().get();
        if channels == self.channels && rate == self.rate {
            return;
        }

        self.channels = channels;
        self.rate = rate;
        self.ramp_frames = (RAMP.as_secs_f64() * rate as f64).round().max(1.) as u32;
        self.frames_left = self.frames_left.min(self.ramp_frames);
        self.delays = vec![[Delay::default(); BANDS]; channels];
        self.recompute();
    }

    fn recompute(&mut self) {
        let rate = self.rate as f32;
        self.preamp_linear = 10f32.powf(self.current_preamp / 20.0);

        for (band_idx, coefficients) in self.coefficients.iter_mut().enumerate() {
            *coefficients = Biquad::from_band(
                self.active_bands[band_idx],
                self.current_gains[band_idx],
                self.current_freqs[band_idx],
                self.current_qs[band_idx],
                rate,
            );
        }

        self.active = self.current_preamp.abs() > 1e-4
            || self
                .current_gains
                .iter()
                .zip(self.active_bands.iter())
                .any(|(gain, band)| band.enabled && gain.abs() > 1e-4);
    }

    fn advance(&mut self) {
        let generation = self.equalizer.generation();
        if generation != self.seen {
            self.seen = generation;
            let eff = self.equalizer.effective();
            let new_gains = eff.bands.map(|b| if b.enabled { b.gain_db } else { 0.0 });
            let new_freqs = eff.bands.map(|b| b.frequency);
            let new_qs = eff.bands.map(|b| b.q);

            self.active_bands = eff.bands;
            self.target_preamp = eff.preamp_db;
            self.target_gains = new_gains;
            self.target_freqs = new_freqs;
            self.target_qs = new_qs;

            let inv = 1.0 / self.ramp_frames as f32;
            self.step_preamp = (self.target_preamp - self.current_preamp) * inv;
            for i in 0..BANDS {
                self.step_gains[i] = (self.target_gains[i] - self.current_gains[i]) * inv;
                self.step_freqs[i] = (self.target_freqs[i] - self.current_freqs[i]) * inv;
                self.step_qs[i] = (self.target_qs[i] - self.current_qs[i]) * inv;
            }
            self.frames_left = self.ramp_frames;
        }

        if self.frames_left == 0 {
            return;
        }
        self.frames_left -= 1;

        if self.frames_left == 0 {
            self.current_preamp = self.target_preamp;
            self.current_gains = self.target_gains;
            self.current_freqs = self.target_freqs;
            self.current_qs = self.target_qs;
        } else {
            self.current_preamp += self.step_preamp;
            for i in 0..BANDS {
                self.current_gains[i] += self.step_gains[i];
                self.current_freqs[i] += self.step_freqs[i];
                self.current_qs[i] += self.step_qs[i];
            }
        }
        self.recompute();
    }
}

impl<I: Source> Iterator for Equalized<I> {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.input.next()?;

        if self.channel == 0 {
            self.resync();
            self.advance();
        }

        let output = match self.active {
            true => {
                let delays = &mut self.delays[self.channel];
                let pre = sample * self.preamp_linear;
                self.coefficients
                    .iter()
                    .zip(delays.iter_mut())
                    .fold(pre, |x, (coefficients, delay)| delay.run(coefficients, x))
            }
            false => sample,
        };

        self.channel += 1;
        if self.channel >= self.channels {
            self.channel = 0;
        }

        Some(output)
    }
}

impl<I: Source> Source for Equalized<I> {
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
        self.input.try_seek(position)?;
        for delays in &mut self.delays {
            *delays = [Delay::default(); BANDS];
        }
        Ok(())
    }
}

// ==============================================================================
// Unit Tests
// ==============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_autoeq_txt() {
        let txt = "\
Preamp: -6.4 dB
Filter 1: ON PK Fc 21 Hz Gain 4.8 dB Q 0.65
Filter 2: ON LSC Fc 105 Hz Gain 2.1 dB Q 0.70
Filter 3: ON HSC Fc 10000 Hz Gain -3.2 dB Q 0.71
Filter 4: OFF PK Fc 1000 Hz Gain 5.0 dB Q 1.41
";
        let profile = AutoEqProfile::from_autoeq_txt("EarAcoustic STA-Hi End", txt).unwrap();
        assert!((profile.preamp_db - (-6.4)).abs() < 1e-4);
        assert_eq!(profile.bands[0].filter_type, FilterType::Peaking);
        assert!((profile.bands[0].frequency - 21.0).abs() < 1e-4);
        assert!((profile.bands[0].gain_db - 4.8).abs() < 1e-4);
        assert!((profile.bands[0].q - 0.65).abs() < 1e-4);

        assert_eq!(profile.bands[1].filter_type, FilterType::LowShelf);
        assert_eq!(profile.bands[2].filter_type, FilterType::HighShelf);
        // Disabled Filter 4 is skipped, so band[3] stays flat 0.0 dB
        assert_eq!(profile.bands[3].gain_db, 0.0);
    }

    #[test]
    fn equalizer_applies_and_exports_autoeq() {
        let eq = Equalizer::default();
        let txt = "Preamp: -4.5 dB\nFilter 1: ON PK Fc 60 Hz Gain 4.5 dB Q 1.20\n";
        let profile = AutoEqProfile::from_autoeq_txt("Custom IEM", txt).unwrap();
        eq.apply_autoeq(&profile);

        assert!(eq.enabled());
        assert!(eq.is_parametric());
        assert!((eq.preamp_db() - (-4.5)).abs() < 1e-4);
        assert!(eq.export_autoeq_txt("Custom IEM").contains("Preamp: -4.5 dB"));
    }
}