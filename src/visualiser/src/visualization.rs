use eframe::egui;

use crate::analysis::{FrequencyBand, HISTORY_LIMIT};

/// Draws band histories with low frequencies at the bottom and newest slices on the right.
pub(crate) fn show(
    ctx: &egui::Context,
    open: &mut bool,
    bands: &[FrequencyBand],
    audio_loaded: bool,
) {
    egui::Window::new("Frequency Visualiser")
        .open(open)
        .default_pos([400.0, 16.0])
        .default_size([820.0, 520.0])
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(10, 9, 15));

            let row_height = rect.height() / bands.len().max(1) as f32;
            for (index, band) in bands.iter().enumerate() {
                let y = rect.bottom() - (index as f32 + 1.0) * row_height;
                let row = egui::Rect::from_min_size(
                    egui::pos2(rect.left(), y),
                    egui::vec2(rect.width(), row_height.max(1.0)),
                );
                painter.rect_filled(row, 0.0, egui::Color32::from_rgb(18, 12, 18));

                let cell_width = rect.width() / HISTORY_LIMIT as f32;
                let start_x = rect.right() - band.history().len() as f32 * cell_width;
                for (column, intensity) in band.history().iter().enumerate() {
                    let x = start_x + column as f32 * cell_width;
                    let cell = egui::Rect::from_min_size(
                        egui::pos2(x, y),
                        egui::vec2(cell_width.ceil(), row_height.max(1.0)),
                    );
                    painter.rect_filled(cell, 0.0, flame_color(*intensity));
                }
            }
            if !audio_loaded {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Open an audio file to visualize its frequency bands",
                    egui::FontId::proportional(17.0),
                    egui::Color32::LIGHT_GRAY,
                );
            }
        });
}

/// Maps clamped intensity to a flame gradient from dark red to pale yellow.
fn flame_color(intensity: f32) -> egui::Color32 {
    let stops = [
        (0.0, [20, 3, 10]),
        (0.34, [112, 12, 14]),
        (0.68, [236, 70, 9]),
        (0.88, [255, 190, 28]),
        (1.0, [255, 250, 210]),
    ];
    let intensity = intensity.clamp(0.0, 1.0);
    let (left, right) = stops
        .windows(2)
        .find(|pair| intensity <= pair[1].0)
        .map(|pair| (pair[0], pair[1]))
        .unwrap_or((stops[3], stops[4]));
    let factor = (intensity - left.0) / (right.0 - left.0);
    let channel = |index: usize| {
        (left.1[index] as f32 + factor * (right.1[index] as f32 - left.1[index] as f32)).round()
            as u8
    };
    egui::Color32::from_rgb(channel(0), channel(1), channel(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flame_colors_are_clamped_and_increase_in_brightness() {
        assert_eq!(flame_color(-1.0), flame_color(0.0));
        assert_eq!(flame_color(2.0), flame_color(1.0));
        let brightness = |color: egui::Color32| {
            u32::from(color.r()) + u32::from(color.g()) + u32::from(color.b())
        };
        for step in 0..100 {
            assert!(
                brightness(flame_color(step as f32 / 100.0))
                    <= brightness(flame_color((step + 1) as f32 / 100.0))
            );
        }
    }
}
