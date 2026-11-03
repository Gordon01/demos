use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
};

use crate::analysis::FFT_SIZE;

const BUFFER_CAPACITY: usize = 48_000;

struct CaptureBuffer {
    samples: VecDeque<f32>,
}

impl Default for CaptureBuffer {
    fn default() -> Self {
        Self {
            samples: VecDeque::with_capacity(BUFFER_CAPACITY),
        }
    }
}

impl CaptureBuffer {
    fn push(&mut self, samples: &[f32]) {
        for sample in samples {
            if self.samples.len() == BUFFER_CAPACITY {
                self.samples.pop_front();
            }
            self.samples.push_back(*sample);
        }
    }

    fn latest_window(&self) -> Option<Vec<f32>> {
        (self.samples.len() >= FFT_SIZE).then(|| {
            self.samples
                .iter()
                .skip(self.samples.len() - FFT_SIZE)
                .copied()
                .collect()
        })
    }

    fn clear(&mut self) {
        self.samples.clear();
    }
}

#[derive(Default)]
struct CaptureState {
    buffer: CaptureBuffer,
    error: Option<String>,
}

#[cfg(target_os = "macos")]
pub(crate) struct SystemAudioCapture {
    started_device:
        Option<cidre::core_audio::hardware::StartedDevice<cidre::core_audio::AggregateDevice>>,
    callback_context: Option<Box<CaptureCallback>>,
    tap: Option<cidre::core_audio::TapGuard>,
    state: Arc<Mutex<CaptureState>>,
    sample_rate: Option<u32>,
}

#[cfg(not(target_os = "macos"))]
pub(crate) struct SystemAudioCapture;

impl SystemAudioCapture {
    pub(crate) fn new() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                started_device: None,
                callback_context: None,
                tap: None,
                state: Arc::new(Mutex::new(CaptureState::default())),
                sample_rate: None,
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            Self
        }
    }

    pub(crate) const fn is_supported() -> bool {
        cfg!(target_os = "macos")
    }

    pub(crate) fn is_running(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.started_device.is_some()
        }

        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    pub(crate) fn sample_rate(&self) -> Option<u32> {
        #[cfg(target_os = "macos")]
        {
            self.sample_rate
        }

        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    pub(crate) fn start(&mut self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            use ca::{aggregate_device_keys as agg_keys, sub_device_keys as sub_keys};
            use cidre::{cf, core_audio as ca, ns};

            if self.started_device.is_some() {
                return Ok(());
            }
            self.clear();
            let output_device = ca::System::default_output_device()
                .map_err(|error| format!("Could not access the default audio output: {error}"))?;
            let output_uid = output_device
                .uid()
                .map_err(|error| format!("Could not identify the default audio output: {error}"))?;
            let tap_description =
                ca::TapDesc::with_mono_global_tap_excluding_processes(&ns::Array::new());
            let tap = tap_description.create_process_tap().map_err(|error| {
                format!(
                    "Could not create the system-audio tap: {error}. \
                     System capture requires macOS 14.2 or newer, Screen & System Audio Recording permission, \
                     and an app bundle with NSAudioCaptureUsageDescription."
                )
            })?;
            let format = tap
                .asbd()
                .map_err(|error| format!("Could not read the audio-tap format: {error}"))?;
            if !format.is_common_f32() || format.channels_per_frame != 1 {
                return Err("The system-audio tap did not provide mono 32-bit PCM.".into());
            }
            if !format.sample_rate.is_finite()
                || format.sample_rate < 1.0
                || format.sample_rate > u32::MAX as f64
            {
                return Err("The system-audio tap provided an invalid sample rate.".into());
            }
            let sample_rate = format.sample_rate.round() as u32;

            let sub_device =
                cf::DictionaryOf::with_keys_values(&[sub_keys::uid()], &[output_uid.as_type_ref()]);
            let sub_tap = cf::DictionaryOf::with_keys_values(
                &[sub_keys::uid()],
                &[tap
                    .uid()
                    .map_err(|error| format!("Could not identify the system-audio tap: {error}"))?
                    .as_type_ref()],
            );
            let aggregate_description = cf::DictionaryOf::with_keys_values(
                &[
                    agg_keys::is_private(),
                    agg_keys::is_stacked(),
                    agg_keys::tap_auto_start(),
                    agg_keys::name(),
                    agg_keys::main_sub_device(),
                    agg_keys::uid(),
                    agg_keys::sub_device_list(),
                    agg_keys::tap_list(),
                ],
                &[
                    cf::Boolean::value_true().as_type_ref(),
                    cf::Boolean::value_false(),
                    cf::Boolean::value_true(),
                    cf::str!(c"Frequency Visualiser Tap"),
                    &output_uid,
                    &cf::Uuid::new().to_cf_string(),
                    &cf::ArrayOf::from_slice(&[sub_device.as_ref()]),
                    &cf::ArrayOf::from_slice(&[sub_tap.as_ref()]),
                ],
            );
            let aggregate_device = ca::AggregateDevice::with_desc(&aggregate_description)
                .map_err(|error| format!("Could not create the audio-tap device: {error}"))?;

            let mut callback_context = Box::new(CaptureCallback {
                state: Arc::clone(&self.state),
            });
            let proc_id = aggregate_device
                .create_io_proc_id(capture_samples, Some(callback_context.as_mut()))
                .map_err(|error| format!("Could not register the audio callback: {error}"))?;
            let started_device = ca::device_start(aggregate_device, Some(proc_id))
                .map_err(|error| format!("Could not start system-audio capture: {error}"))?;

            self.sample_rate = Some(sample_rate);
            self.callback_context = Some(callback_context);
            self.tap = Some(tap);
            self.started_device = Some(started_device);
            Ok(())
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err("System-audio capture is only supported on macOS.".into())
        }
    }

    pub(crate) fn stop(&mut self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let result = if let Some(started_device) = self.started_device.take() {
                started_device
                    .stop()
                    .map(drop)
                    .map_err(|error| format!("Could not stop system-audio capture: {error}"))
            } else {
                Ok(())
            };
            self.callback_context = None;
            self.tap = None;
            self.sample_rate = None;
            self.clear();
            result
        }

        #[cfg(not(target_os = "macos"))]
        {
            Ok(())
        }
    }

    pub(crate) fn latest_samples(&self) -> Option<(Vec<f32>, u32)> {
        #[cfg(target_os = "macos")]
        {
            let sample_rate = self.sample_rate?;
            lock_state(&self.state)
                .buffer
                .latest_window()
                .map(|samples| (samples, sample_rate))
        }

        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    pub(crate) fn take_error(&self) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            lock_state(&self.state).error.take()
        }

        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    pub(crate) fn clear(&mut self) {
        #[cfg(target_os = "macos")]
        {
            let mut state = lock_state(&self.state);
            state.buffer.clear();
            state.error = None;
        }
    }
}

#[cfg(target_os = "macos")]
struct CaptureCallback {
    state: Arc<Mutex<CaptureState>>,
}

#[cfg(target_os = "macos")]
extern "C" fn capture_samples(
    _device: cidre::core_audio::Device,
    _now: &cidre::cat::AudioTimeStamp,
    input_data: &cidre::cat::AudioBufList<1>,
    _input_time: &cidre::cat::AudioTimeStamp,
    _output_data: &mut cidre::cat::AudioBufList<1>,
    _output_time: &cidre::cat::AudioTimeStamp,
    context: Option<&mut CaptureCallback>,
) -> cidre::os::Status {
    let Some(context) = context else {
        return cidre::os::Status(-50);
    };
    let Some(buffer) = input_data.as_slice().first() else {
        return cidre::os::Status::NO_ERR;
    };
    let byte_len = buffer.data_bytes_size as usize;
    if byte_len == 0 {
        return cidre::os::Status::NO_ERR;
    }
    if buffer.data.is_null() || !byte_len.is_multiple_of(std::mem::size_of::<f32>()) {
        record_error(
            &context.state,
            "The system-audio tap returned an invalid sample buffer.".into(),
        );
        return cidre::os::Status::NO_ERR;
    }
    if buffer.number_channels != 1 {
        record_error(
            &context.state,
            "The system-audio tap returned an unexpected channel count.".into(),
        );
        return cidre::os::Status::NO_ERR;
    }

    let sample_count = byte_len / std::mem::size_of::<f32>();
    // The tap format was checked as native-endian, mono, 32-bit floating-point PCM before start.
    let samples = unsafe { std::slice::from_raw_parts(buffer.data.cast::<f32>(), sample_count) };
    if samples.iter().any(|sample| !sample.is_finite()) {
        record_error(
            &context.state,
            "The system-audio tap returned invalid samples.".into(),
        );
        return cidre::os::Status::NO_ERR;
    }
    let mut state = lock_state(&context.state);
    state.buffer.push(samples);
    cidre::os::Status::NO_ERR
}

#[cfg(target_os = "macos")]
fn lock_state(state: &Mutex<CaptureState>) -> MutexGuard<'_, CaptureState> {
    match state.lock() {
        Ok(state) => state,
        Err(poisoned) => {
            let mut state = poisoned.into_inner();
            state.error = Some("System-audio capture state became unavailable.".into());
            state
        }
    }
}

#[cfg(target_os = "macos")]
fn record_error(state: &Mutex<CaptureState>, error: String) {
    let mut state = lock_state(state);
    if state.error.is_none() {
        state.error = Some(error);
    }
}

#[cfg(target_os = "macos")]
impl Drop for SystemAudioCapture {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("{error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_buffer_keeps_a_bounded_latest_window() {
        let mut buffer = CaptureBuffer::default();
        buffer.push(&vec![0.25; BUFFER_CAPACITY]);
        buffer.push(&vec![0.5; FFT_SIZE]);

        let window = buffer.latest_window().unwrap();
        assert_eq!(window.len(), FFT_SIZE);
        assert!(window.iter().all(|sample| *sample == 0.5));
        assert_eq!(buffer.samples.len(), BUFFER_CAPACITY);

        buffer.clear();
        assert!(buffer.latest_window().is_none());
    }
}
