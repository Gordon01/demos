use std::time::{Duration, Instant};

use eframe::egui;

use crate::{analysis::Analyzer, audio::Playback, system_audio::SystemAudioCapture, visualization};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AudioSource {
    #[default]
    File,
    System,
}

/// Coordinates audio playback, frequency analysis, settings, and visualization windows.
pub(crate) struct VisualiserApp {
    playback: Playback,
    system_capture: SystemAudioCapture,
    source: AudioSource,
    analyzer: Analyzer,
    settings_open: bool,
    visualisation_open: bool,
    last_frame: Instant,
    status: String,
    band_count_setting: usize,
    min_frequency_setting: f32,
    max_frequency_setting: f32,
    layout: visualization::Layout,
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
            system_capture: SystemAudioCapture::new(),
            source: AudioSource::File,
            analyzer: Analyzer::new(36, 20.0, 16_000.0),
            settings_open: true,
            visualisation_open: true,
            last_frame: Instant::now(),
            status,
            band_count_setting: 36,
            min_frequency_setting: 20.0,
            max_frequency_setting: 16_000.0,
            layout: visualization::Layout::default(),
        }
    }

    /// Draws settings and applies file selection, playback controls, and band changes.
    fn draw_controls(&mut self, ctx: &egui::Context) {
        let mut settings_open = self.settings_open;
        let mut selected_source = self.source;
        egui::Window::new("Audio and Settings")
            .open(&mut settings_open)
            .default_pos([16.0, 16.0])
            .default_size([380.0, 480.0])
            .show(ctx, |ui| {
                egui::ComboBox::from_label("Audio source")
                    .selected_text(match selected_source {
                        AudioSource::File => "Audio file",
                        AudioSource::System => "System output",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut selected_source, AudioSource::File, "Audio file");
                        ui.add_enabled_ui(SystemAudioCapture::is_supported(), |ui| {
                            ui.selectable_value(
                                &mut selected_source,
                                AudioSource::System,
                                "System output",
                            );
                        });
                    });
                if !SystemAudioCapture::is_supported() {
                    ui.small("System output capture requires macOS 14.2 or newer.");
                }

                if selected_source == AudioSource::File
                    && ui.button("Open audio file...").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Audio files", &["wav", "mp3", "flac", "ogg"])
                        .pick_file()
                {
                    match self.playback.load(&path) {
                        Ok(()) => {
                            self.switch_source(AudioSource::File);
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

                match selected_source {
                    AudioSource::File => {
                        ui.horizontal(|ui| {
                            let button = if self.playback.is_playing() {
                                "Pause"
                            } else {
                                "Play"
                            };
                            if ui
                                .add_enabled(
                                    self.playback.audio().is_some(),
                                    egui::Button::new(button),
                                )
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
                                .add_enabled(
                                    self.playback.audio().is_some(),
                                    egui::Button::new("Stop"),
                                )
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
                    }
                    AudioSource::System => {
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    SystemAudioCapture::is_supported()
                                        && !self.system_capture.is_running(),
                                    egui::Button::new("Start capture"),
                                )
                                .clicked()
                            {
                                self.analyzer.clear_history();
                                match self.system_capture.start() {
                                    Ok(()) => self.status = "Capturing system output.".into(),
                                    Err(error) => self.status = error,
                                }
                            }
                            if ui
                                .add_enabled(
                                    self.system_capture.is_running(),
                                    egui::Button::new("Stop capture"),
                                )
                                .clicked()
                            {
                                self.analyzer.clear_history();
                                self.status = self
                                    .system_capture
                                    .stop()
                                    .map(|()| "System-audio capture stopped.".into())
                                    .unwrap_or_else(|error| error);
                            }
                        });
                        if let Some(sample_rate) = self.system_capture.sample_rate() {
                            ui.label(format!("Capture sample rate: {sample_rate} Hz"));
                        }
                    }
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
                ui.heading("Visualisation");
                egui::ComboBox::from_label("Animation direction")
                    .selected_text(self.layout.direction.label())
                    .show_ui(ui, |ui| {
                        for direction in visualization::ScrollDirection::ALL {
                            ui.selectable_value(
                                &mut self.layout.direction,
                                direction,
                                direction.label(),
                            );
                        }
                    });
                ui.checkbox(&mut self.layout.reverse_bands, "Reverse band order");

                ui.separator();
                ui.label(&self.status);
                ui.checkbox(&mut self.visualisation_open, "Show visualisation");
            });
        self.settings_open = settings_open;
        self.switch_source(selected_source);
    }

    fn switch_source(&mut self, source: AudioSource) {
        if self.source == source {
            return;
        }
        let stop_error = self
            .system_capture
            .is_running()
            .then(|| self.system_capture.stop())
            .and_then(Result::err);
        if source == AudioSource::System {
            self.playback.stop();
            self.status = stop_error
                .unwrap_or_else(|| "System output selected. Start capture to visualize it.".into());
        } else if self.playback.audio().is_none() {
            self.status = stop_error.unwrap_or_else(|| "Open an audio file to begin.".into());
        } else {
            self.status = stop_error.unwrap_or_else(|| "Audio file selected.".into());
        }
        self.source = source;
        self.analyzer.clear_history();
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

        match self.source {
            AudioSource::File => {
                if self.playback.is_playing()
                    && let Some(audio) = self.playback.audio()
                {
                    let center_sample =
                        (self.playback.position() * audio.sample_rate() as f32) as usize;
                    let intensities = self.analyzer.analyze(
                        audio.mono_samples(),
                        center_sample,
                        audio.sample_rate(),
                    );
                    self.analyzer.advance(&intensities, delta_seconds);
                }
            }
            AudioSource::System => {
                if self.system_capture.is_running()
                    && let Some((samples, sample_rate)) = self.system_capture.latest_samples()
                {
                    let intensities =
                        self.analyzer
                            .analyze(&samples, samples.len() / 2, sample_rate);
                    self.analyzer.advance(&intensities, delta_seconds);
                }
                if let Some(error) = self.system_capture.take_error() {
                    self.status = match self.system_capture.stop() {
                        Ok(()) => error,
                        Err(stop_error) => format!("{error} {stop_error}"),
                    };
                    self.analyzer.clear_history();
                }
            }
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
                match self.source {
                    AudioSource::File => self.playback.audio().is_some(),
                    AudioSource::System => self.system_capture.is_running(),
                },
                self.layout,
            );
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}
