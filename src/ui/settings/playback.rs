//! GPUI Playback Settings View Component.
//!
//! Provides the "Playback" tab settings section in GPUI featuring:
//! - Spatial Audio Toggle Switch (Enable / Disable).
//! - Spatial Mode Selection Dropdown (Dolby Atmos Passthrough, Custom HRTF, Binaural Crossfeed).
//! - Interactive description panel detailing the active acoustic virtualization algorithm.
//! - Direct reactive binding to `SpatialAudioProcessor` and the app playback engine.

use gpui::prelude::*;
use gpui::{
    div, AnyElement, Context, ElementId, Entity, EventEmitter, IntoElement,
    ParentElement, Render, SharedString, Styled, View, Window, px, rgb,
};

use crate::audio::engine::SpatialAudioProcessor;
use crate::audio::spatial::SpatialMode;

// ==============================================================================
// Playback Settings UI Events & Component State
// ==============================================================================

/// Events emitted when playback settings change.
#[derive(Clone, Debug)]
pub enum PlaybackSettingsEvent {
    SpatialAudioToggled(bool),
    SpatialModeChanged(SpatialMode),
    SoundstageWidthChanged(f32),
}

/// Standalone Playback Settings GPUI component.
pub struct PlaybackSettingsView {
    processor: SpatialAudioProcessor,
    dropdown_open: bool,
}

impl PlaybackSettingsView {
    /// Create a new Playback Settings view bound to the audio engine processor.
    pub fn new(processor: SpatialAudioProcessor, _cx: &mut Context<Self>) -> Self {
        Self {
            processor,
            dropdown_open: false,
        }
    }

    /// Access the bound spatial audio processor.
    pub fn processor(&self) -> &SpatialAudioProcessor {
        &self.processor
    }

    /// Toggles spatial audio processing on or off.
    pub fn toggle_spatial_audio(&mut self, cx: &mut Context<Self>) {
        let new_state = !self.processor.is_enabled();
        self.processor.set_enabled(new_state);
        cx.emit(PlaybackSettingsEvent::SpatialAudioToggled(new_state));
        cx.notify();
    }

    /// Selects a new spatial audio mode.
    pub fn select_mode(&mut self, mode: SpatialMode, cx: &mut Context<Self>) {
        self.processor.set_mode(mode);
        self.dropdown_open = false;
        cx.emit(PlaybackSettingsEvent::SpatialModeChanged(mode));
        cx.notify();
    }

    /// Toggles the mode dropdown menu visibility.
    pub fn toggle_dropdown(&mut self, cx: &mut Context<Self>) {
        self.dropdown_open = !self.dropdown_open;
        cx.notify();
    }

    /// Helper to render a settings row.
    fn render_row(
        &self,
        title: &'static str,
        description: &'static str,
        control: impl IntoElement,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .py_3()
            .border_b_1()
            .border_color(rgb(0x27272a))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(0xf4f4f5))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0xa1a1aa))
                            .max_w(px(460.0))
                            .child(description),
                    ),
            )
            .child(control)
    }

    /// Renders the toggle switch element.
    fn render_switch(&self, enabled: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let bg_color = if enabled { rgb(0xef4444) } else { rgb(0x3f3f46) };
        let thumb_x = if enabled { px(20.0) } else { px(2.0) };

        div()
            .id(ElementId::Name("spatial-audio-switch".into()))
            .w(px(42.0))
            .h(px(24.0))
            .rounded_full()
            .bg(bg_color)
            .cursor_pointer()
            .relative()
            .p_0p5()
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_spatial_audio(cx);
            }))
            .child(
                div()
                    .w(px(20.0))
                    .h(px(20.0))
                    .rounded_full()
                    .bg(rgb(0xffffff))
                    .absolute()
                    .top(px(2.0))
                    .left(thumb_x),
            )
    }

    /// Renders the mode dropdown picker.
    fn render_dropdown(
        &self,
        current_mode: SpatialMode,
        is_open: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut menu_items = Vec::new();
        if is_open {
            for mode in SpatialMode::ALL {
                let is_selected = mode == current_mode;
                let item_bg = if is_selected { rgb(0x27272a) } else { rgb(0x18181b) };

                menu_items.push(
                    div()
                        .id(ElementId::NamedInteger("mode-item".into(), mode as u64))
                        .flex()
                        .flex_col()
                        .px_3()
                        .py_2()
                        .bg(item_bg)
                        .hover(|s| s.bg(rgb(0x3f3f46)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_mode(mode, cx);
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(if is_selected {
                                            gpui::FontWeight::SEMIBOLD
                                        } else {
                                            gpui::FontWeight::NORMAL
                                        })
                                        .text_color(if is_selected {
                                            rgb(0xef4444)
                                        } else {
                                            rgb(0xf4f4f5)
                                        })
                                        .child(mode.display_name()),
                                )
                                .child(if is_selected { "✓" } else { "" }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0xa1a1aa))
                                .child(mode.description()),
                        ),
                );
            }
        }

        div()
            .relative()
            .child(
                div()
                    .id(ElementId::Name("mode-dropdown-trigger".into()))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .bg(rgb(0x27272a))
                    .border_1()
                    .border_color(rgb(0x3f3f46))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x3f3f46)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_dropdown(cx);
                    }))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xf4f4f5))
                            .child(current_mode.display_name()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0xa1a1aa))
                            .child(if is_open { "▲" } else { "▼" }),
                    ),
            )
            .child(if is_open {
                div()
                    .absolute()
                    .right_0()
                    .top(px(38.0))
                    .w(px(320.0))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x3f3f46))
                    .bg(rgb(0x18181b))
                    .shadow_lg()
                    .children(menu_items)
            } else {
                div()
            })
    }
}

impl EventEmitter<PlaybackSettingsEvent> for PlaybackSettingsView {}

impl Render for PlaybackSettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_enabled = self.processor.is_enabled();
        let current_mode = self.processor.mode();
        let is_open = self.dropdown_open;

        div()
            .flex()
            .flex_col()
            .w_full()
            .gap_4()
            .p_4()
            .bg(rgb(0x121214))
            .rounded_lg()
            // Header
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(0xf4f4f5))
                            .child("Spatial Audio & 3D Soundstage"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0xa1a1aa))
                            .child("Transform standard stereo music into an immersive three-dimensional holographic listening experience."),
                    ),
            )
            // Spatial Audio Toggle Switch Row
            .child(self.render_row(
                "Spatial Audio Processing",
                "Enable real-time 3D soundstage virtualization and acoustic crossfeed filters.",
                self.render_switch(is_enabled, cx),
            ))
            // Spatial Mode Dropdown Row (Only active when Spatial Audio is enabled)
            .child(if is_enabled {
                self.render_row(
                    "Spatial Processing Mode",
                    "Choose between Windows Dolby Atmos passthrough, binaural HRTF, or Bauer crossfeed.",
                    self.render_dropdown(current_mode, is_open, cx),
                )
            } else {
                div()
            })
            // Status Info Card
            .child(if is_enabled {
                div()
                    .flex()
                    .flex_col()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0x1e1e24))
                    .border_1()
                    .border_color(rgb(0x27272a))
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(0xef4444))
                            .child(format!("Active Virtualization: {}", current_mode.display_name())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0xa1a1aa))
                            .child(current_mode.description()),
                    )
            } else {
                div()
            })
    }
}
