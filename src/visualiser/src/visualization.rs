use std::collections::VecDeque;

use eframe::egui;

use crate::analysis::{FrequencyBand, HISTORY_LIMIT};

const LANE_BACKGROUND: egui::Color32 = egui::Color32::from_rgb(18, 12, 18);

/// Direction in which the band history moves across the plot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ScrollDirection {
    Left,
    Right,
    #[default]
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

            if !bands.is_empty() {
                let image = history_image(bands.iter().map(FrequencyBand::history));
                let mut mesh = egui::Mesh::with_texture(history_texture(ctx, image));
                let corners = [
                    rect.left_top(),
                    rect.right_top(),
                    rect.right_bottom(),
                    rect.left_bottom(),
                ];
                for (pos, uv) in corners.into_iter().zip(quad_uvs(layout)) {
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos,
                        uv,
                        color: egui::Color32::WHITE,
                    });
                }
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                painter.add(mesh);
            }
            if !audio_loaded {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Start playback or system-audio capture to visualize frequency bands",
                    egui::FontId::proportional(17.0),
                    egui::Color32::LIGHT_GRAY,
                );
            }
        });
}

/// Uploads the image into a texture kept in egui memory, skipping unchanged images.
fn history_texture(ctx: &egui::Context, image: egui::ColorImage) -> egui::TextureId {
    let id = egui::Id::unique("band-history-texture");
    let options = egui::TextureOptions::NEAREST;
    let cached = ctx
        .data(|data| data.get_temp::<(egui::TextureHandle, std::sync::Arc<egui::ColorImage>)>(id));
    let texture = match cached {
        Some((texture, previous)) if *previous == image => return texture.id(),
        Some((mut texture, _)) => {
            texture.set(image.clone(), options);
            texture
        }
        None => ctx.load_texture("band-history", image.clone(), options),
    };
    ctx.data_mut(|data| data.insert_temp(id, (texture.clone(), std::sync::Arc::new(image))));
    texture.id()
}

/// Builds an image with one row per band (lowest first) and the newest slice in the last column.
fn history_image<'a>(
    histories: impl ExactSizeIterator<Item = &'a VecDeque<f32>>,
) -> egui::ColorImage {
    let height = histories.len();
    let mut pixels = Vec::with_capacity(height * HISTORY_LIMIT);
    for history in histories {
        let shown = history.len().min(HISTORY_LIMIT);
        pixels.extend(std::iter::repeat_n(LANE_BACKGROUND, HISTORY_LIMIT - shown));
        pixels.extend(
            history
                .range(history.len() - shown..)
                .map(|intensity| flame_color(*intensity)),
        );
    }
    egui::ColorImage::new([HISTORY_LIMIT, height], pixels)
}

/// Returns texture coordinates for the plot's top-left, top-right, bottom-right and
/// bottom-left corners; `u` runs oldest to newest and `v` lowest to highest band.
fn quad_uvs(layout: Layout) -> [egui::Pos2; 4] {
    [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(x, y): (f32, f32)| {
        let (time, band) = match layout.direction {
            ScrollDirection::Left => (x, 1.0 - y),
            ScrollDirection::Right => (1.0 - x, 1.0 - y),
            ScrollDirection::Up => (y, x),
            ScrollDirection::Down => (1.0 - y, x),
        };
        let band = if layout.reverse_bands {
            1.0 - band
        } else {
            band
        };
        egui::pos2(time, band)
    })
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

    const CORNERS: [(f32, f32); 4] = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];

    fn layout(direction: ScrollDirection, reverse_bands: bool) -> Layout {
        Layout {
            direction,
            reverse_bands,
        }
    }

    /// Returns the texture coordinate at a normalised plot corner.
    fn uv_at(layout: Layout, x: f32, y: f32) -> egui::Pos2 {
        let index = CORNERS.iter().position(|corner| *corner == (x, y)).unwrap();
        quad_uvs(layout)[index]
    }

    #[test]
    fn newest_slice_appears_opposite_to_motion() {
        for reverse in [false, true] {
            for direction in ScrollDirection::ALL {
                let layout = layout(direction, reverse);
                let newest_corners = match direction {
                    ScrollDirection::Left => [(1.0, 0.0), (1.0, 1.0)],
                    ScrollDirection::Right => [(0.0, 0.0), (0.0, 1.0)],
                    ScrollDirection::Up => [(0.0, 1.0), (1.0, 1.0)],
                    ScrollDirection::Down => [(0.0, 0.0), (1.0, 0.0)],
                };
                for (x, y) in newest_corners {
                    assert_eq!(uv_at(layout, x, y).x, 1.0, "{direction:?} {reverse}");
                    assert_eq!(uv_at(layout, 1.0 - x, 1.0 - y).x, 0.0, "{direction:?}");
                }
            }
        }
    }

    #[test]
    fn band_order_follows_orientation_and_reversal() {
        let low = |direction, reverse, x, y| uv_at(layout(direction, reverse), x, y).y == 0.0;
        assert!(low(ScrollDirection::Left, false, 0.0, 1.0));
        assert!(low(ScrollDirection::Left, false, 1.0, 1.0));
        assert!(low(ScrollDirection::Right, true, 0.0, 0.0));
        assert!(low(ScrollDirection::Right, true, 1.0, 0.0));
        assert!(low(ScrollDirection::Up, false, 0.0, 0.0));
        assert!(low(ScrollDirection::Up, false, 0.0, 1.0));
        assert!(low(ScrollDirection::Down, true, 1.0, 0.0));
        assert!(low(ScrollDirection::Down, true, 1.0, 1.0));
    }

    #[test]
    fn history_image_aligns_newest_slices_to_the_last_column() {
        let full: VecDeque<f32> = (0..HISTORY_LIMIT + 5).map(|_| 0.0).chain([1.0]).collect();
        let histories = [VecDeque::from([1.0, 0.0]), VecDeque::new(), full];
        let image = history_image(histories.iter());
        assert_eq!(image.size, [HISTORY_LIMIT, 3]);
        let row = |index: usize| &image.pixels[index * HISTORY_LIMIT..(index + 1) * HISTORY_LIMIT];
        assert!(
            row(0)[..HISTORY_LIMIT - 2]
                .iter()
                .all(|pixel| *pixel == LANE_BACKGROUND)
        );
        assert_eq!(
            row(0)[HISTORY_LIMIT - 2..],
            [flame_color(1.0), flame_color(0.0)]
        );
        assert!(row(1).iter().all(|pixel| *pixel == LANE_BACKGROUND));
        assert_eq!(row(2)[HISTORY_LIMIT - 1], flame_color(1.0));
        assert_eq!(row(2)[0], flame_color(0.0));
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
