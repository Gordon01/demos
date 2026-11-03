use std::{collections::VecDeque, sync::Arc};

use rustfft::{Fft, FftPlanner, num_complex::Complex};

const FFT_SIZE: usize = 4096;
pub(crate) const HISTORY_LIMIT: usize = 1200;
const ACTIVE_THRESHOLD: f32 = 0.025;
const MAX_SCROLL_SPEED: f32 = 320.0;

/// Holds one frequency interval and its independently scrolling intensity history.
pub(crate) struct FrequencyBand {
    low_hz: f32,
    high_hz: f32,
    history: VecDeque<f32>,
    movement_remainder: f32,
}

impl FrequencyBand {
    fn new(low_hz: f32, high_hz: f32) -> Self {
        Self {
            low_hz,
            high_hz,
            history: VecDeque::new(),
            movement_remainder: 0.0,
        }
    }

    pub(crate) fn history(&self) -> &VecDeque<f32> {
        &self.history
    }

    /// Adds history at an intensity-dependent speed, leaving quiet bands stationary.
    fn advance(&mut self, intensity: f32, delta_seconds: f32) {
        if intensity <= ACTIVE_THRESHOLD {
            return;
        }
        self.movement_remainder += intensity * MAX_SCROLL_SPEED * delta_seconds;
        while self.movement_remainder >= 1.0 {
            self.history.push_back(intensity);
            self.movement_remainder -= 1.0;
        }
        while self.history.len() > HISTORY_LIMIT {
            self.history.pop_front();
        }
    }
}

/// Reuses FFT buffers to measure logarithmic frequency bands and update their histories.
pub(crate) struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex<f32>>,
    input: Vec<Complex<f32>>,
    bands: Vec<FrequencyBand>,
}

impl Analyzer {
    pub(crate) fn new(band_count: usize, min_frequency: f32, max_frequency: f32) -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        Self {
            scratch: vec![Complex::default(); fft.get_inplace_scratch_len()],
            input: vec![Complex::default(); FFT_SIZE],
            fft,
            bands: create_bands(band_count, min_frequency, max_frequency),
        }
    }

    /// Rebuilds the logarithmic bands, discarding their previous histories.
    pub(crate) fn configure(&mut self, band_count: usize, min_frequency: f32, max_frequency: f32) {
        self.bands = create_bands(band_count, min_frequency, max_frequency);
    }

    pub(crate) fn bands(&self) -> &[FrequencyBand] {
        &self.bands
    }

    /// Clears recorded intensities and fractional movement without changing the bands.
    pub(crate) fn clear_history(&mut self) {
        for band in &mut self.bands {
            band.history.clear();
            band.movement_remainder = 0.0;
        }
    }

    /// Sums Hann-windowed FFT magnitudes into bands and returns intensities in 0..=1.
    pub(crate) fn analyze(
        &mut self,
        samples: &[f32],
        center_sample: usize,
        sample_rate: u32,
    ) -> Vec<f32> {
        self.input.fill(Complex::default());
        let start = center_sample.saturating_sub(FFT_SIZE / 2);
        for (index, value) in self.input.iter_mut().enumerate() {
            if let Some(sample) = samples.get(start + index) {
                let hann = 0.5
                    - 0.5
                        * (2.0 * std::f32::consts::PI * index as f32 / (FFT_SIZE - 1) as f32).cos();
                value.re = sample * hann;
            }
        }
        self.fft
            .process_with_scratch(&mut self.input, &mut self.scratch);

        let mut magnitudes = vec![0.0; self.bands.len()];
        let frequency_per_bin = sample_rate as f32 / FFT_SIZE as f32;
        for (bin, value) in self.input.iter().take(FFT_SIZE / 2).enumerate().skip(1) {
            let frequency = bin as f32 * frequency_per_bin;
            if let Some((index, _)) = self
                .bands
                .iter()
                .enumerate()
                .find(|(_, band)| frequency >= band.low_hz && frequency < band.high_hz)
            {
                magnitudes[index] += value.norm();
            }
        }

        let full_scale_magnitude = FFT_SIZE as f32 * 0.25;
        magnitudes
            .into_iter()
            .map(|magnitude| (magnitude / full_scale_magnitude).clamp(0.0, 1.0).sqrt())
            .collect()
    }

    /// Advances each band's history independently using elapsed time in seconds.
    pub(crate) fn advance(&mut self, intensities: &[f32], delta_seconds: f32) {
        for (band, intensity) in self.bands.iter_mut().zip(intensities) {
            band.advance(*intensity, delta_seconds);
        }
    }
}

/// Divides the requested frequency range into ascending, logarithmically spaced bands.
fn create_bands(count: usize, min_frequency: f32, max_frequency: f32) -> Vec<FrequencyBand> {
    let ratio = max_frequency / min_frequency;
    (0..count)
        .map(|index| {
            let low_hz = min_frequency * ratio.powf(index as f32 / count as f32);
            let high_hz = min_frequency * ratio.powf((index + 1) as f32 / count as f32);
            FrequencyBand::new(low_hz, high_hz)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_are_logarithmically_spaced_and_ascending() {
        let bands = create_bands(4, 20.0, 320.0);
        assert_eq!(bands.len(), 4);
        assert!((bands[0].low_hz - 20.0).abs() < 0.001);
        assert!((bands[3].high_hz - 320.0).abs() < 0.01);
        for pair in bands.windows(2) {
            assert!((pair[0].high_hz - pair[1].low_hz).abs() < 0.001);
            assert!((pair[0].high_hz / pair[0].low_hz - 2.0).abs() < 0.001);
        }
    }

    #[test]
    fn silent_audio_has_no_band_intensity() {
        let mut analyzer = Analyzer::new(12, 20.0, 16_000.0);
        let intensities = analyzer.analyze(&vec![0.0; FFT_SIZE], FFT_SIZE / 2, 48_000);
        assert_eq!(intensities.len(), 12);
        assert!(intensities.iter().all(|intensity| *intensity == 0.0));
    }

    #[test]
    fn sine_wave_intensity_is_assigned_to_its_frequency_band() {
        let sample_rate = 48_000;
        let target_bin = 38;
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|index| {
                (2.0 * std::f32::consts::PI * target_bin as f32 * index as f32 / FFT_SIZE as f32)
                    .sin()
            })
            .collect();
        let mut analyzer = Analyzer::new(36, 20.0, 16_000.0);
        let intensities = analyzer.analyze(&samples, FFT_SIZE / 2, sample_rate);
        let peak_band = intensities
            .iter()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(right.1))
            .map(|(index, _)| index)
            .unwrap();
        let frequency = target_bin as f32 * sample_rate as f32 / FFT_SIZE as f32;
        let expected_band = analyzer
            .bands
            .iter()
            .position(|band| frequency >= band.low_hz && frequency < band.high_hz)
            .unwrap();
        assert_eq!(peak_band, expected_band);
        assert!(intensities[peak_band] > 0.9);

        let quiet_samples: Vec<f32> = samples.iter().map(|sample| sample * 0.0001).collect();
        let quiet_intensities = analyzer.analyze(&quiet_samples, FFT_SIZE / 2, sample_rate);
        assert!(quiet_intensities[expected_band] < ACTIVE_THRESHOLD);
    }

    #[test]
    fn inactive_frequency_band_does_not_scroll() {
        let mut band = FrequencyBand::new(20.0, 40.0);
        band.advance(ACTIVE_THRESHOLD, 1.0);
        assert!(band.history.is_empty());
        band.advance(1.0, 0.01);
        assert!(!band.history.is_empty());
    }

    #[test]
    fn histories_move_independently_and_remain_bounded() {
        let mut analyzer = Analyzer::new(3, 20.0, 320.0);
        analyzer.advance(&[1.0, 0.5, 0.0], 1.0);
        assert_eq!(analyzer.bands()[0].history().len(), 320);
        assert_eq!(analyzer.bands()[1].history().len(), 160);
        assert!(analyzer.bands()[2].history().is_empty());
        analyzer.advance(&[1.0, 1.0, 0.0], 10.0);
        assert_eq!(analyzer.bands()[0].history().len(), HISTORY_LIMIT);
        analyzer.clear_history();
        assert!(
            analyzer
                .bands()
                .iter()
                .all(|band| band.history().is_empty())
        );
        assert!(
            analyzer
                .bands
                .iter()
                .all(|band| band.movement_remainder == 0.0)
        );
        analyzer.configure(8, 40.0, 8000.0);
        assert_eq!(analyzer.bands().len(), 8);
    }
}
