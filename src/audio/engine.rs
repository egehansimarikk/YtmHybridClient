//! Audio Engine Spatial Audio Integration Module.
//!
//! Exposes the thread-safe `SpatialAudioProcessor` that can be shared between
//! GPUI UI state threads and real-time audio playback threads (`cpal` / `rodio` / WASAPI).
//!
//! Supports seamless dynamic mode switching during active playback without stopping
//! the stream or introducing audible glitches.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::spatial::{
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    AudioError, BinauralCrossfeedEngine, CrossFader, CustomHrtfEngine,
    DEFAULT_RAMP_DURATION, DolbyAtmosPassthroughEngine, SpatialMode,
};

/// Internal rendering DSP state held under a lightweight mutex.
#[derive(Clone, Debug)]
struct DspState {
    sample_rate: u32,
    atmos: DolbyAtmosPassthroughEngine,
    hrtf: CustomHrtfEngine,
    crossfeed: BinauralCrossfeedEngine,
    current_mode: SpatialMode,
    target_mode: SpatialMode,
    enabled: bool,
    target_enabled: bool,
    fader: CrossFader,
}

impl DspState {
    fn new(sample_rate: u32, enabled: bool, mode: SpatialMode) -> Self {
        Self {
            sample_rate,
            atmos: DolbyAtmosPassthroughEngine::new(),
            hrtf: CustomHrtfEngine::new(sample_rate, 1.0),
            crossfeed: BinauralCrossfeedEngine::new(sample_rate, 1.0),
            current_mode: mode,
            target_mode: mode,
            enabled,
            target_enabled: enabled,
            fader: CrossFader::new(DEFAULT_RAMP_DURATION, sample_rate),
        }
    }

    fn set_sample_rate(&mut self, sample_rate: u32) {
        if self.sample_rate != sample_rate {
            self.sample_rate = sample_rate;
            self.hrtf.set_sample_rate(sample_rate);
            self.crossfeed.set_sample_rate(sample_rate);
            self.fader.set_sample_rate(DEFAULT_RAMP_DURATION, sample_rate);
        }
    }

    fn request_mode(&mut self, new_mode: SpatialMode) {
        if self.target_mode != new_mode {
            self.target_mode = new_mode;
            self.fader.trigger();
        }
    }

    fn request_enabled(&mut self, on: bool) {
        if self.target_enabled != on {
            self.target_enabled = on;
            self.fader.trigger();
        }
    }

    #[inline(always)]
    fn process_mode(&mut self, mode: SpatialMode, left: f32, right: f32) -> (f32, f32) {
        match mode {
            SpatialMode::DolbyAtmosPassthrough => self.atmos.process_frame(left, right),
            SpatialMode::CustomHRTF => self.hrtf.process_frame(left, right),
            SpatialMode::BinauralCrossfeed => self.crossfeed.process_frame(left, right),
        }
    }

    #[inline(always)]
    fn process_frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.fader.is_fading() {
            if !self.enabled {
                return (left, right);
            }
            return self.process_mode(self.current_mode, left, right);
        }

        // Active crossfade ramp
        let (prev_gain, next_gain) = self.fader.next_gains();
        let (prev_l, prev_r) = if self.enabled {
            self.process_mode(self.current_mode, left, right)
        } else {
            (left, right)
        };
        let (next_l, next_r) = if self.target_enabled {
            self.process_mode(self.target_mode, left, right)
        } else {
            (left, right)
        };

        let out_l = prev_l * prev_gain + next_l * next_gain;
        let out_r = prev_r * prev_gain + next_r * next_gain;

        if !self.fader.is_fading() {
            self.enabled = self.target_enabled;
            self.current_mode = self.target_mode;
        }

        (out_l, out_r)
    }
}

/// Atomic fast-path handles for lock-free query from GUI or audio thread.
struct SharedHandle {
    enabled: AtomicBool,
    mode_id: AtomicU32,
    generation: AtomicU32,
    soundstage_width: AtomicU32,
    crossfeed_amount: AtomicU32,
}

impl SharedHandle {
    fn new(enabled: bool, mode: SpatialMode) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            mode_id: AtomicU32::new(mode as u32),
            generation: AtomicU32::new(1),
            soundstage_width: AtomicU32::new(1.0f32.to_bits()),
            crossfeed_amount: AtomicU32::new(1.0f32.to_bits()),
        }
    }
}

/// Thread-safe Spatial Audio Processor.
///
/// Wraps real-time DSP pipelines and exposes safe, lock-free/atomic methods
/// for dynamic runtime adjustments from GPUI views without interrupting playback.
#[derive(Clone)]
pub struct SpatialAudioProcessor {
    shared: Arc<SharedHandle>,
    dsp: Arc<Mutex<DspState>>,
}

impl SpatialAudioProcessor {
    /// Create a new thread-safe processor.
    pub fn new(enabled: bool, mode: SpatialMode) -> Self {
        let shared = Arc::new(SharedHandle::new(enabled, mode));
        let dsp = Arc::new(Mutex::new(DspState::new(44100, enabled, mode)));
        Self { shared, dsp }
    }

    /// Whether spatial audio processing is enabled.
    pub fn is_enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::Relaxed)
    }

    /// Toggle spatial audio on/off with an audible equal-power crossfade.
    pub fn set_enabled(&self, enabled: bool) {
        self.shared.enabled.store(enabled, Ordering::Relaxed);
        self.shared.generation.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut dsp) = self.dsp.lock() {
            dsp.request_enabled(enabled);
        }
    }

    /// Active spatial audio mode.
    pub fn mode(&self) -> SpatialMode {
        match self.shared.mode_id.load(Ordering::Relaxed) {
            0 => SpatialMode::DolbyAtmosPassthrough,
            1 => SpatialMode::CustomHRTF,
            2 => SpatialMode::BinauralCrossfeed,
            _ => SpatialMode::DolbyAtmosPassthrough,
        }
    }

    /// Dynamically switch spatial audio modes during active playback without stopping the stream.
    pub fn set_mode(&self, mode: SpatialMode) {
        self.shared.mode_id.store(mode as u32, Ordering::Relaxed);
        self.shared.generation.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut dsp) = self.dsp.lock() {
            dsp.request_mode(mode);
        }
    }

    /// Sets the Custom HRTF soundstage width factor (0.5 to 2.0).
    pub fn set_soundstage_width(&self, width: f32) {
        self.shared
            .soundstage_width
            .store(width.clamp(0.5, 2.0).to_bits(), Ordering::Relaxed);
        if let Ok(mut dsp) = self.dsp.lock() {
            dsp.hrtf.set_width(width);
        }
    }

    /// Sets the Binaural Crossfeed intensity amount (0.0 to 1.0).
    pub fn set_crossfeed_amount(&self, amount: f32) {
        self.shared
            .crossfeed_amount
            .store(amount.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        if let Ok(mut dsp) = self.dsp.lock() {
            dsp.crossfeed.set_amount(amount);
        }
    }

    /// Returns the required WASAPI stream flags.
    pub fn wasapi_stream_flags(&self) -> u32 {
        if self.is_enabled() && self.mode() == SpatialMode::DolbyAtmosPassthrough {
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY
        } else {
            0
        }
    }

    /// Prepares the internal DSP pipelines for the specified sample rate.
    pub fn prepare(&self, sample_rate: u32) -> Result<(), AudioError> {
        if sample_rate == 0 {
            return Err(AudioError::UnsupportedFormat {
                channels: 2,
                sample_rate,
            });
        }
        let mut dsp = self
            .dsp
            .lock()
            .map_err(|e| AudioError::LockError(e.to_string()))?;
        dsp.set_sample_rate(sample_rate);
        Ok(())
    }

    /// Process a stereo frame: `(in_left, in_right) -> (out_left, out_right)`.
    #[inline(always)]
    pub fn process_frame(&self, left: f32, right: f32) -> (f32, f32) {
        if let Ok(mut dsp) = self.dsp.lock() {
            dsp.process_frame(left, right)
        } else {
            (left, right)
        }
    }

    /// Process an interleaved stereo PCM buffer in-place (`[L0, R0, L1, R1, ...]`).
    pub fn process_interleaved(&self, buffer: &mut [f32]) -> Result<(), AudioError> {
        if buffer.len() % 2 != 0 {
            return Err(AudioError::ChannelMismatch {
                expected: 2,
                found: buffer.len() % 2,
            });
        }

        let mut dsp = self
            .dsp
            .lock()
            .map_err(|e| AudioError::LockError(e.to_string()))?;

        for frame in buffer.chunks_exact_mut(2) {
            let (out_l, out_r) = dsp.process_frame(frame[0], frame[1]);
            frame[0] = out_l;
            frame[1] = out_r;
        }

        Ok(())
    }
}

impl Default for SpatialAudioProcessor {
    fn default() -> Self {
        Self::new(false, SpatialMode::DolbyAtmosPassthrough)
    }
}
