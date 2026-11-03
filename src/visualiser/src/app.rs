use std::time::{Duration, Instant};

use eframe::egui;

use crate::{analysis::Analyzer, audio::Playback, visualization};

/// Coordinates audio playback, frequency analysis, settings, and visualization windows.
pub(crate) struct VisualiserApp {
    playback: Playback,
    analyzer: Analyzer,
    settings_open: bool,
    visualisation_open: bool,
    last_frame: Instant,
    status: String,
    band_count_setting: usize,
    min_frequency_setting: f32,
    max_frequency_setting: f32,
}

impl VisualiserApp {
    pub(crate) fn new() -> Self {
        let playback = Playback::new();
        let status = playback
            .output_error()
            .unwrap_or("Open an audio file to begin.")
            .to_owned();
        Self {
            playback,
            analyzer: Analyzer::new(36, 20.0, 16_000.0),
            settings_open: true,
            visualisation_open: true,
            last_frame: Instant::now(),
            status,
            band_count_setting: 36,
            min_frequency_setting: 20.0,
            max_frequency_setting: 16_000.0,
        }
    }

    /// Draws settings and applies file selection, playback controls, and band changes.
    fn draw_controls(&mut self, ctx: &egui::Context) {
        let mut settings_open = self.settings_open;
        egui::Window::new("Audio and Settings")
            .open(&mut settings_open)
            .default_pos([16.0, 16.0])
            .default_size([360.0, 420.0])
            .show(ctx, |ui| {
                if ui.button("Open audio file...").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Audio files", &["wav", "mp3", "flac", "ogg"])
                        .pick_file()
                {
                    match self.playback.load(&path) {
                        Ok(()) => {
                            self.analyzer.clear_history();
                            if let Some(audio) = self.playback.audio() {
                                self.status = format!(
                                    "Loaded {} ({:.1}s, {} Hz).",
                                    path.display(),
                                    audio.duration(),
                                    audio.sample_rate()
                                );
                            }
                        }
                        Err(error) => self.status = error,
                    }
                }

                ui.horizontal(|ui| {
                    let button = if self.playback.is_playing() {
                        "Pause"
                    } else {
                        "Play"
                    };
                    if ui
                        .add_enabled(self.playback.audio().is_some(), egui::Button::new(button))
                        .clicked()
                    {
                        if self.playback.is_playing() {
                            self.playback.pause();
                            self.status = "Paused.".into();
                        } else {
                            match self.playback.play() {
                                Ok(()) => self.status = "Playing.".into(),
                                Err(error) => self.status = error,
                            }
                        }
                    }
                    if ui
                        .add_enabled(self.playback.audio().is_some(), egui::Button::new("Stop"))
                        .clicked()
                    {
                        self.playback.stop();
                        self.status = "Stopped.".into();
                    }
                });

                if let Some((duration, path)) = self
                    .playback
                    .audio()
                    .map(|audio| (audio.duration(), audio.path().display().to_string()))
                {
                    let mut seek = self.playback.position().min(duration);
                    if ui
                        .add(
                            egui::Slider::new(&mut seek, 0.0..=duration)
                                .show_value(false)
                                .text(format!(
                                    "{:.1}s / {:.1}s",
                                    self.playback.position(),
                                    duration
                                )),
                        )
                        .changed()
                        && let Err(error) = self.playback.seek(seek)
                    {
                        self.status = error;
                    }
                    ui.label(path);
                }

                ui.separator();
                ui.heading("Frequency bands");
                ui.add(egui::Slider::new(&mut self.band_count_setting, 8..=80).text("Band count"));
                ui.add(
                    egui::Slider::new(&mut self.min_frequency_setting, 20.0..=1000.0)
                        .logarithmic(true)
                        .text("Lowest frequency (Hz)"),
                );
                ui.add(
                    egui::Slider::new(&mut self.max_frequency_setting, 1000.0..=20_000.0)
                        .logarithmic(true)
                        .text("Highest frequency (Hz)"),
                );
                let valid_range = self.min_frequency_setting < self.max_frequency_setting;
                if ui
                    .add_enabled(valid_range, egui::Button::new("Apply frequency settings"))
                    .clicked()
                {
                    self.analyzer.configure(
                        self.band_count_setting,
                        self.min_frequency_setting,
                        self.max_frequency_setting,
                    );
                }
                if !valid_range {
                    ui.colored_label(
                        egui::Color32::LIGHT_RED,
                        "The highest frequency must exceed the lowest.",
                    );
                }

                ui.separator();
                ui.label(&self.status);
                ui.checkbox(&mut self.visualisation_open, "Show visualisation");
            });
        self.settings_open = settings_open;
    }
}

impl eframe::App for VisualiserApp {
    /// Updates playback and active histories before drawing the current frame.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let delta_seconds = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        let was_playing = self.playback.is_playing();
        self.playback.update();
        if was_playing && !self.playback.is_playing() {
            self.status = "Playback finished.".into();
        }

        if self.playback.is_playing()
            && let Some(audio) = self.playback.audio()
        {
            let center_sample = (self.playback.position() * audio.sample_rate() as f32) as usize;
            let intensities =
                self.analyzer
                    .analyze(audio.mono_samples(), center_sample, audio.sample_rate());
            self.analyzer.advance(&intensities, delta_seconds);
        }

        egui::TopBottomPanel::top("window_controls").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if !self.settings_open && ui.button("Open settings").clicked() {
                    self.settings_open = true;
                }
                if !self.visualisation_open && ui.button("Open visualisation").clicked() {
                    self.visualisation_open = true;
                }
            });
        });
        self.draw_controls(ctx);
        if self.visualisation_open {
            visualization::show(
                ctx,
                &mut self.visualisation_open,
                self.analyzer.bands(),
                self.playback.audio().is_some(),
            );
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}
