use eframe::egui;

use crate::analysis::{FrequencyBand, HISTORY_LIMIT};

/// Direction in which the band history moves across the plot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ScrollDirection {
    #[default]
    Left,
    Right,
    Up,
    Down,
}

impl ScrollDirection {
    pub(crate) const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Up, Self::Down];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Up => "Up",
            Self::Down => "Down",
        }
    }

    fn is_horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// User-adjustable plot orientation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) direction: ScrollDirection,
    /// Puts low frequencies at the top (horizontal) or right (vertical) instead.
    pub(crate) reverse_bands: bool,
}

/// Draws band histories, moving older slices away from the edge where new slices appear.
pub(crate) fn show(
    ctx: &egui::Context,
    open: &mut bool,
    bands: &[FrequencyBand],
    audio_loaded: bool,
    layout: Layout,
) {
    egui::Window::new("Frequency Visualiser")
        .open(open)
        .default_pos([400.0, 16.0])
        .default_size([820.0, 520.0])
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(10, 9, 15));

            for (index, band) in bands.iter().enumerate() {
                let lane = band_lane(rect, layout, index, bands.len());
                painter.rect_filled(lane, 0.0, egui::Color32::from_rgb(18, 12, 18));

                let history = band.history();
                for (column, intensity) in history.iter().enumerate() {
                    let age = history.len() - 1 - column;
                    let cell = history_cell(lane, layout.direction, age);
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

/// Returns the strip of `rect` occupied by one band; low bands start at bottom or left by default.
fn band_lane(rect: egui::Rect, layout: Layout, index: usize, count: usize) -> egui::Rect {
    let count = count.max(1) as f32;
    let index = index as f32;
    if layout.direction.is_horizontal() {
        let height = rect.height() / count;
        let y = if layout.reverse_bands {
            rect.top() + index * height
        } else {
            rect.bottom() - (index + 1.0) * height
        };
        egui::Rect::from_min_size(
            egui::pos2(rect.left(), y),
            egui::vec2(rect.width(), height.max(1.0)),
        )
    } else {
        let width = rect.width() / count;
        let x = if layout.reverse_bands {
            rect.right() - (index + 1.0) * width
        } else {
            rect.left() + index * width
        };
        egui::Rect::from_min_size(
            egui::pos2(x, rect.top()),
            egui::vec2(width.max(1.0), rect.height()),
        )
    }
}

/// Returns the cell within a band lane for a slice `age` frames older than the newest one.
fn history_cell(lane: egui::Rect, direction: ScrollDirection, age: usize) -> egui::Rect {
    let age = age as f32;
    // Cells are rounded up to avoid gaps; the extra size extends toward older slices.
    let (x, y) = (lane.x_range(), lane.y_range());
    let step = |length: f32| length / HISTORY_LIMIT as f32;
    let (x, y) = match direction {
        ScrollDirection::Left => {
            let max = lane.right() - age * step(lane.width());
            (egui::Rangef::new(max - step(lane.width()).ceil(), max), y)
        }
        ScrollDirection::Right => {
            let min = lane.left() + age * step(lane.width());
            (egui::Rangef::new(min, min + step(lane.width()).ceil()), y)
        }
        ScrollDirection::Up => {
            let max = lane.bottom() - age * step(lane.height());
            (x, egui::Rangef::new(max - step(lane.height()).ceil(), max))
        }
        ScrollDirection::Down => {
            let min = lane.top() + age * step(lane.height());
            (x, egui::Rangef::new(min, min + step(lane.height()).ceil()))
        }
    };
    egui::Rect::from_x_y_ranges(x, y)
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

    fn plot() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 600.0))
    }

    fn layout(direction: ScrollDirection, reverse_bands: bool) -> Layout {
        Layout {
            direction,
            reverse_bands,
        }
    }

    #[test]
    fn newest_slice_appears_opposite_to_motion() {
        let rect = plot();
        for direction in ScrollDirection::ALL {
            let lane = band_lane(rect, layout(direction, false), 0, 4);
            let newest = history_cell(lane, direction, 0);
            let older = history_cell(lane, direction, 10);
            let (origin, newest_edge, older_edge) = match direction {
                ScrollDirection::Left => (rect.right(), newest.right(), older.right()),
                ScrollDirection::Right => (rect.left(), newest.left(), older.left()),
                ScrollDirection::Up => (rect.bottom(), newest.bottom(), older.bottom()),
                ScrollDirection::Down => (rect.top(), newest.top(), older.top()),
            };
            assert_eq!(newest_edge, origin, "{direction:?}");
            assert!(
                (older_edge - origin).abs() > (newest_edge - origin).abs(),
                "{direction:?}"
            );
        }
    }

    #[test]
    fn band_order_follows_orientation_and_reversal() {
        let rect = plot();
        let low = |direction, reverse| band_lane(rect, layout(direction, reverse), 0, 4);
        assert_eq!(low(ScrollDirection::Left, false).bottom(), rect.bottom());
        assert_eq!(low(ScrollDirection::Right, true).top(), rect.top());
        assert_eq!(low(ScrollDirection::Up, false).left(), rect.left());
        assert_eq!(low(ScrollDirection::Down, true).right(), rect.right());
        assert_eq!(low(ScrollDirection::Up, false).height(), rect.height());
        assert_eq!(low(ScrollDirection::Left, false).width(), rect.width());
    }

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
