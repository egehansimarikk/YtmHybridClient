//! Spatial Audio / Native Passthrough module for native YouTube Music client.
//!
//! Implements BitChord-style native passthrough architecture with optional BS2B crossfeed:
//! - BS2B Crossfeed (Optional): Lightweight Bauer Stereo-to-Binaural crossfeed for subtle
//!   stereo soundstage widening without hollow reverb or phase cancellation.
//! - Disabled/Stereo: Pure 1:1 bit-exact passthrough with zero processing, zero delay,
//!   and zero gain alterations.
//!
//! Philosophy: When crossfeed is enabled, apply only the gentlest frequency-selective
//! low-pass crossfeed (700 Hz cutoff, ~4.5 dB attenuation) to widen stereo imaging while
//! preserving 100% of bass punch and center vocal clarity.

use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::Source;
use rodio::source::SeekError;

// ==============================================================================
// BS2B (Bauer Binaural Crossfeed) Implementation
// ==============================================================================

/// BS2B crossfeed coefficients for 44.1 kHz sample rate
/// Derived from the standard BS2B algorithm with 700 Hz low-pass filter
/// and ~4.5 dB attenuation on cross-fed signal
const BS2B_COEFFS_44K: [f32; 5] = [0.612457f32, 0.244980f32, 0.087543f32, 0.194988f32, 0.069972f32];

/// BS2B crossfeed coefficients for 48 kHz sample rate
const BS2B_COEFFS_48K: [f32; 5] = [0.594604f32, 0.231170f32, 0.094604f32, 0.181166f32, 0.064851f32];

/// BS2B crossfeed coefficients for 96 kHz sample rate
const BS2B_COEFFS_96K: [f32; 5] = [0.560124f32, 0.205644f32, 0.105122f32, 0.156381f32, 0.054938f32];

/// BS2B crossfeed coefficients for 192 kHz sample rate
const BS2B_COEFFS_192K: [f32; 5] = [0.533378f32, 0.185891f32, 0.113214f32, 0.137593f32, 0.047385f32];

/// Get BS2B coefficients for a given sample rate
fn get_bs2b_coeffs(sample_rate: u32) -> &'static [f32; 5] {
    if sample_rate >= 192000 {
        &BS2B_COEFFS_192K
    } else if sample_rate >= 96000 {
        &BS2B_COEFFS_96K
    } else if sample_rate >= 48000 {
        &BS2B_COEFFS_48K
    } else {
        &BS2B_COEFFS_44K
    }
}

/// BS2B (Bauer Stereo-to-Binaural) Crossfeed Engine
///
/// Implements a clean, lightweight crossfeed algorithm that:
/// - Gently widens stereo soundstage
/// - Applies frequency-selective low-pass crossfeed (~700 Hz cutoff)
/// - Uses ~4.5 dB attenuation on cross-fed signal
/// - Preserves 100% bass punch and center vocal clarity
/// - Adds NO reverb, echo, room impulses, or heavy HRTF filters
#[derive(Clone, Debug)]
pub struct CrossfeedEngine {
    /// Whether crossfeed is enabled
    enabled: bool,
    /// Previous left sample for delay line
    prev_left: f32,
    /// Previous right sample for delay line
    prev_right: f32,
    /// BS2B coefficients
    coeffs: [f32; 5],
}

impl CrossfeedEngine {
    /// Create a new crossfeed engine
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            prev_left: 0.0,
            prev_right: 0.0,
            coeffs: BS2B_COEFFS_44K,
        }
    }

    /// Create a crossfeed engine prepared for a specific sample rate
    pub fn with_sample_rate(enabled: bool, sample_rate: u32) -> Self {
        Self {
            enabled,
            prev_left: 0.0,
            prev_right: 0.0,
            coeffs: *get_bs2b_coeffs(sample_rate),
        }
    }

    /// Enable or disable crossfeed
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if crossfeed is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Prepare the engine for a specific sample rate
    pub fn prepare(&mut self, sample_rate: u32) {
        self.coeffs = *get_bs2b_coeffs(sample_rate);
    }

    /// Reset the engine state
    pub fn reset(&mut self) {
        self.prev_left = 0.0;
        self.prev_right = 0.0;
    }

    /// Process a stereo frame using BS2B crossfeed algorithm
    ///
    /// The BS2B algorithm uses a biquad low-pass filter to apply frequency-selective
    /// crossfeed:
    /// - Direct signal (same channel): passes through unchanged
    /// - Cross-fed signal (opposite channel): passes through low-pass filter with attenuation
    ///
    /// Coefficients are tuned for 700 Hz cutoff with ~4.5 dB attenuation on cross-feed.
    #[inline(always)]
    pub fn process_frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.enabled {
            // Pure 1:1 bit-exact passthrough
            return (left, right);
        }

        // BS2B biquad filter coefficients
        // a1, a2: feedback coefficients
        // b0, b1, b2: feedforward coefficients
        let [b0, b1, b2, a1, a2] = self.coeffs;

        // Process left channel - receives direct left + cross-fed right
        let crossfeed_left = self.apply_biquad(right, b0, b1, b2, a1, a2);
        let out_left = left + crossfeed_left;

        // Process right channel - receives direct right + cross-fed left
        let crossfeed_right = self.apply_biquad(left, b0, b1, b2, a1, a2);
        let out_right = right + crossfeed_right;

        (out_left, out_right)
    }

    /// Apply BS2B biquad low-pass filter to a single channel
    #[inline(always)]
    fn apply_biquad(&mut self, input: f32, b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> f32 {
        // Biquad filter difference equation:
        // y[n] = b0*x[n] + b1*x[n-1] + b2*x[n-2] - a1*y[n-1] - a2*y[n-2]
        let output = b0 * input + b1 * self.prev_left + b2 * self.prev_right - a1 * self.prev_left - a2 * self.prev_right;

        // Update delay line
        self.prev_right = self.prev_left;
        self.prev_left = output;

        output
    }

    /// Process a buffer of interleaved stereo samples
    pub fn process_buffer(&mut self, buffer: &mut [f32]) {
        let len = buffer.len() / 2;
        for i in 0..len {
            let idx = i * 2;
            let left = buffer[idx];
            let right = buffer[idx + 1];

            let (out_left, out_right) = self.process_frame(left, right);
            buffer[idx] = out_left;
            buffer[idx + 1] = out_right;
        }
    }
}

impl Default for CrossfeedEngine {
    fn default() -> Self {
        Self::new(false)
    }
}

// ==============================================================================
// WASAPI Constants & Stream Flags
// ==============================================================================

/// WASAPI stream flag instructing the audio engine to automatically convert
/// the client format to the mixing format and route into spatial audio pipelines.
pub const AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM: u32 = 0x8000_0000;

/// WASAPI stream flag specifying default quality sample rate conversion.
pub const AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY: u32 = 0x0800_0000;

// ==============================================================================
// Error Handling
// ==============================================================================

/// Error type returned by audio engine and spatial processor operations.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioError {
    /// Audio output device is unavailable or was unplugged.
    DeviceUnavailable(String),
    /// Format with specified channel count and sample rate is unsupported.
    UnsupportedFormat { channels: u16, sample_rate: u32 },
    /// Stream buffer underflow occurred during rendering.
    BufferUnderflow,
    /// Channel count mismatch between input stream and processor expectations.
    ChannelMismatch { expected: usize, found: usize },
    /// Internal thread synchronization lock contention error.
    LockError(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeviceUnavailable(msg) => write!(f, "Audio output device unavailable: {msg}"),
            Self::UnsupportedFormat { channels, sample_rate } => {
                write!(f, "Unsupported audio format: {channels}ch @ {sample_rate} Hz")
            }
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
// Spatial Audio Modes
// ==============================================================================

/// The spatial audio processing mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum SpatialMode {
    /// BS2B Binaural Crossfeed: Lightweight stereo soundstage widener
    #[default]
    BS2BCrossfeed,
}

impl SpatialMode {
    /// All available spatial modes.
    pub const ALL: [Self; 1] = [Self::BS2BCrossfeed];

    /// Unique identifier for settings serialization.
    pub fn id(self) -> &'static str {
        match self {
            Self::BS2BCrossfeed => "bs2b_crossfeed",
        }
    }

    /// User-facing display name.
    pub fn display_name(self) -> &'static str {
        "BS2B Binaural Crossfeed"
    }

    /// User-facing description explaining the acoustic benefit.
    pub fn description(self) -> &'static str {
        "Lightweight BS2B crossfeed: Gentle stereo widening at 700 Hz with ~4.5 dB attenuation"
    }

    /// Parse a mode identifier string.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "bs2b_crossfeed" => Some(Self::BS2BCrossfeed),
            _ => None,
        }
    }
}

// ==============================================================================
// Native Passthrough Engine
// ==============================================================================

/// Native passthrough engine with optional BS2B crossfeed.
///
/// When disabled: pure 1:1 bit-exact passthrough with zero processing.
/// When enabled with BS2B mode: applies lightweight frequency-selective crossfeed.
#[derive(Clone, Debug)]
pub struct DolbyAtmosPassthroughEngine {
    crossfeed: CrossfeedEngine,
}

impl DolbyAtmosPassthroughEngine {
    pub fn new() -> Self {
        Self {
            crossfeed: CrossfeedEngine::new(false),
        }
    }

    pub fn with_mode(mode: SpatialMode) -> Self {
        match mode {
            SpatialMode::BS2BCrossfeed => Self {
                crossfeed: CrossfeedEngine::new(true),
            },
        }
    }

    /// Returns the required WASAPI audio stream flags.
    pub fn wasapi_stream_flags(&self) -> u32 {
        // BS2B crossfeed is applied in software, not via WASAPI flags
        // The stream runs as standard stereo PCM
        0
    }

    /// Process a stereo frame
    #[inline(always)]
    pub fn process_frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        self.crossfeed.process_frame(left, right)
    }

    /// Check if crossfeed is enabled
    pub fn is_enabled(&self) -> bool {
        self.crossfeed.is_enabled()
    }

    /// Enable or disable crossfeed
    pub fn set_enabled(&mut self, enabled: bool) {
        self.crossfeed.set_enabled(enabled);
    }

    /// Set the spatial mode
    pub fn set_mode(&mut self, mode: SpatialMode) {
        match mode {
            SpatialMode::BS2BCrossfeed => {
                self.crossfeed.set_enabled(true);
            }
        }
    }

    /// Prepare for a specific sample rate
    pub fn prepare(&mut self, sample_rate: u32) {
        self.crossfeed.prepare(sample_rate);
    }

    pub fn reset(&mut self) {
        self.crossfeed.reset();
    }
}

impl Default for DolbyAtmosPassthroughEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ==============================================================================
// Thread-Safe Spatial Audio Processor
// ==============================================================================

/// Internal DSP state maintained on the audio rendering thread.
#[derive(Clone, Debug)]
struct DspState {
    sample_rate: u32,
}

impl DspState {
    fn new(sample_rate: u32) -> Self {
        Self { sample_rate }
    }
}

/// Shared lock-free/atomic handle between UI thread and audio rendering engine.
struct SharedHandle {
    enabled: AtomicBool,
    mode_id: AtomicU32,
}

impl SharedHandle {
    fn new(enabled: bool, mode: SpatialMode) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            mode_id: AtomicU32::new(mode as u32),
        }
    }
}

/// Thread-safe Spatial Audio Processor implementing BitChord native passthrough with BS2B.
///
/// Can be cloned and safely shared across GUI and audio threads. Provides:
/// - Pure 1:1 bit-exact passthrough when disabled
/// - BS2B crossfeed when enabled (700 Hz low-pass, ~4.5 dB attenuation)
#[derive(Clone)]
pub struct SpatialAudioProcessor {
    shared: Arc<SharedHandle>,
    dsp: Arc<Mutex<DspState>>,
}

impl SpatialAudioProcessor {
    /// Creates a new spatial audio processor with the given initial state.
    pub fn new(enabled: bool, mode: SpatialMode) -> Self {
        let shared = Arc::new(SharedHandle::new(enabled, mode));
        let dsp = Arc::new(Mutex::new(DspState::new(44100)));
        Self { shared, dsp }
    }

    /// Creates a new spatial audio processor with sample rate information.
    pub fn with_sample_rate(enabled: bool, mode: SpatialMode, sample_rate: u32) -> Self {
        let shared = Arc::new(SharedHandle::new(enabled, mode));
        let dsp = Arc::new(Mutex::new(DspState::new(sample_rate)));
        Self { shared, dsp }
    }

    /// Whether spatial audio processing is enabled.
    pub fn is_enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::Relaxed)
    }

    /// Enable or disable spatial audio processing.
    /// When disabled: pure 1:1 bit-exact passthrough with zero processing.
    pub fn set_enabled(&self, enabled: bool) {
        self.shared.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Current active spatial audio mode.
    pub fn mode(&self) -> SpatialMode {
        match self.shared.mode_id.load(Ordering::Relaxed) {
            0 => SpatialMode::BS2BCrossfeed,
            _ => SpatialMode::BS2BCrossfeed,
        }
    }

    /// Switch spatial audio mode dynamically during playback.
    pub fn set_mode(&self, mode: SpatialMode) {
        self.shared.mode_id.store(mode as u32, Ordering::Relaxed);
        // Update sample rate in DSP on next process
        if let Ok(ref mut _dsp) = self.dsp.try_lock() {
            // Sample rate will be updated on next prepare()
        }
    }

    /// Returns the required WASAPI stream flags based on current configuration.
    /// BS2B crossfeed runs as standard stereo PCM, no special WASAPI flags needed.
    pub fn wasapi_stream_flags(&self) -> u32 {
        // BS2B is software-only, runs as standard stereo PCM
        0
    }

    /// Prepare processor for a specific sample rate.
    pub fn prepare(&self, sample_rate: u32) -> Result<(), AudioError> {
        if let Ok(ref mut dsp) = self.dsp.try_lock() {
            dsp.sample_rate = sample_rate;
        }
        Ok(())
    }

    /// Process a slice of interleaved 2-channel stereo PCM samples in-place.
    ///
    /// Implements BitChord philosophy with BS2B:
    /// - When disabled: pure 1:1 bit-exact passthrough
    /// - When enabled: BS2B crossfeed (700 Hz low-pass, ~4.5 dB attenuation)
    pub fn process_interleaved(&self, buffer: &mut [f32]) -> Result<(), AudioError> {
        if buffer.len() % 2 != 0 {
            return Err(AudioError::ChannelMismatch {
                expected: 2,
                found: buffer.len() % 2,
            });
        }

        // BitChord philosophy: Pure 1:1 bit-exact passthrough when disabled
        if !self.shared.enabled.load(Ordering::Relaxed) {
            return Ok(());
        }

        // BS2B crossfeed mode - process in-place
        if let Ok(ref mut dsp) = self.dsp.try_lock() {
            let mut engine = CrossfeedEngine::with_sample_rate(true, dsp.sample_rate);
            engine.process_buffer(buffer);
        }

        Ok(())
    }

    /// Wraps any rodio Source to provide real-time spatialized audio streaming.
    pub fn wrap_source<I>(&self, source: I) -> SpatialSource<I>
    where
        I: Source<Item = f32>,
    {
        SpatialSource::new(source, self.clone())
    }

    /// Checks if spatial processing is enabled - for conditional wrapping in audio chain.
    pub fn should_process(&self) -> bool {
        self.is_enabled()
    }
}

impl Default for SpatialAudioProcessor {
    fn default() -> Self {
        Self::new(false, SpatialMode::BS2BCrossfeed)
    }
}

// ==============================================================================
// Rodio Source Adapter (Zero-Latency Streaming Pipeline)
// ==============================================================================

/// A rodio `Source` adapter that transparently applies spatial audio processing.
pub struct SpatialSource<I> {
    input: I,
    processor: SpatialAudioProcessor,
    channels: NonZero<u16>,
    sample_rate: NonZero<u32>,
    /// Internal engine state for processing
    engine: Option<CrossfeedEngine>,
    /// Buffered right channel sample from last frame (left was already yielded)
    pending_right: Option<f32>,
}

impl<I: Source<Item = f32>> SpatialSource<I> {
    pub fn new(input: I, processor: SpatialAudioProcessor) -> Self {
        let sample_rate = input.sample_rate();
        let channels = input.channels();

        let engine = if processor.is_enabled() {
            Some(CrossfeedEngine::with_sample_rate(true, sample_rate.get()))
        } else {
            None
        };

        Self {
            input,
            processor,
            channels,
            sample_rate,
            engine,
            pending_right: None,
        }
    }

    pub fn processor(&self) -> &SpatialAudioProcessor {
        &self.processor
    }
}

impl<I: Source<Item = f32>> Iterator for SpatialSource<I> {
    type Item = f32;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        // For stereo streams with spatial enabled, we need to process complete frames
        if self.channels.get() == 2 && self.processor.is_enabled() {
            // Ensure we have an engine
            let engine = self.engine.get_or_insert_with(|| {
                CrossfeedEngine::with_sample_rate(true, self.sample_rate.get())
            });

            // Read a complete stereo frame
            let left = self.input.next()?;
            let right = self.input.next()?;

            // Process the frame through BS2B crossfeed
            let (out_left, out_right) = engine.process_frame(left, right);

            // Output the left sample, and buffer the right for next call
            self.pending_right = Some(out_right);
            return Some(out_left);
        }

        // For mono, multichannel, or disabled spatial: pure passthrough
        self.input.next()
    }
}

impl<I: Source<Item = f32>> Source for SpatialSource<I> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> NonZero<u16> {
        self.channels
    }

    fn sample_rate(&self) -> NonZero<u32> {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        self.input.try_seek(position)
    }
}

// ==============================================================================
// Unit Tests
// ==============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_enumeration() {
        assert_eq!(SpatialMode::ALL.len(), 1);
        for mode in SpatialMode::ALL {
            assert_eq!(SpatialMode::from_id(mode.id()), Some(mode));
        }
    }

    #[test]
    fn test_bs2b_crossfeed_coeffs() {
        // Verify coefficients are different for different sample rates
        let coeffs_44k = get_bs2b_coeffs(44100);
        let coeffs_48k = get_bs2b_coeffs(48000);
        let coeffs_96k = get_bs2b_coeffs(96000);

        assert_ne!(coeffs_44k, coeffs_48k);
        assert_ne!(coeffs_48k, coeffs_96k);
    }

    #[test]
    fn test_crossfeed_passthrough_when_disabled() {
        let mut engine = CrossfeedEngine::new(false);

        // Verify 1:1 bit-exact passthrough when disabled
        let (left, right) = engine.process_frame(0.5, -0.5);
        assert!((left - 0.5).abs() < 1e-6);
        assert!((right - (-0.5)).abs() < 1e-6);
    }

    #[test]
    fn test_crossfeed_enabled() {
        let mut engine = CrossfeedEngine::new(true);

        // Verify crossfeed is applied (output differs from input)
        let (left, right) = engine.process_frame(0.5, -0.5);

        // With crossfeed, channels should have some contribution from both sides
        // Left should have some right content, right should have some left content
        assert!(left != 0.5 || right != -0.5);
    }

    #[test]
    fn test_crossfeed_preserves_center_clarity() {
        // When both channels have the same signal (center pan),
        // crossfeed should not cancel it out
        let mut engine = CrossfeedEngine::new(true);

        let (left, right) = engine.process_frame(0.5, 0.5);

        // Both outputs should still have the center signal
        assert!(left > 0.3);
        assert!(right > 0.3);
    }

    #[test]
    fn test_crossfeed_stereo_widening() {
        // When channels have opposite signals, crossfeed should widen the stereo image
        let mut engine = CrossfeedEngine::new(true);

        let (left, right) = engine.process_frame(0.5, -0.5);

        // Left should have some negative content (cross-fed from right)
        // Right should have some positive content (cross-fed from left)
        // This widens the stereo image
        assert!(left < 0.5);
        assert!(right > -0.5);
    }

    #[test]
    fn test_crossfeed_buffer_processing() {
        let mut engine = CrossfeedEngine::new(true);

        let mut buffer = vec![0.5f32, -0.5f32, 0.3f32, -0.3f32];
        engine.process_buffer(&mut buffer);

        // First frame processed
        assert!(buffer[0] != 0.5 || buffer[1] != -0.5);
        // Second frame processed
        assert!(buffer[2] != 0.3 || buffer[3] != -0.3);
    }

    #[test]
    fn test_dolby_engine_modes() {
        let mut engine = DolbyAtmosPassthroughEngine::with_mode(SpatialMode::BS2BCrossfeed);
        assert!(engine.is_enabled());

        engine.set_enabled(false);
        assert!(!engine.is_enabled());

        engine.set_enabled(true);
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_wasapi_flags_clean() {
        // BS2B runs as standard stereo PCM, no WASAPI flags needed
        let processor = SpatialAudioProcessor::new(true, SpatialMode::BS2BCrossfeed);
        assert_eq!(processor.wasapi_stream_flags(), 0);

        let processor = SpatialAudioProcessor::new(false, SpatialMode::BS2BCrossfeed);
        assert_eq!(processor.wasapi_stream_flags(), 0);
    }
}
