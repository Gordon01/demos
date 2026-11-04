use std::{
    fs::File,
    io::{BufReader, Read, Seek},
    path::{Path, PathBuf},
};

use rodio::{
    ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source,
    buffer::SamplesBuffer,
};

/// Stores interleaved samples for playback and a mono mix for frequency analysis.
pub(crate) struct AudioFile {
    path: PathBuf,
    samples: Vec<f32>,
    mono_samples: Vec<f32>,
    channels: ChannelCount,
    sample_rate: SampleRate,
}

impl AudioFile {
    fn open(path: &Path) -> Result<Self, String> {
        let file =
            File::open(path).map_err(|error| format!("Could not open audio file: {error}"))?;
        Self::decode(path, BufReader::new(file))
    }

    /// Decodes complete audio frames and averages their channels into mono samples.
    fn decode<R: Read + Seek + Send + Sync + 'static>(
        path: &Path,
        reader: R,
    ) -> Result<Self, String> {
        let decoder = Decoder::new(reader)
            .map_err(|error| format!("Could not decode audio file: {error}"))?;
        let channels = decoder.channels();
        let sample_rate = decoder.sample_rate();

        let samples: Vec<f32> = decoder.collect();
        if samples.is_empty() || !samples.len().is_multiple_of(usize::from(channels.get())) {
            return Err("The audio file contains no complete audio frames.".into());
        }
        let mono_samples = samples
            .chunks_exact(usize::from(channels.get()))
            .map(|frame| frame.iter().sum::<f32>() / f32::from(channels.get()))
            .collect();
        Ok(Self {
            path: path.to_owned(),
            samples,
            mono_samples,
            channels,
            sample_rate,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn mono_samples(&self) -> &[f32] {
        &self.mono_samples
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate.get()
    }

    pub(crate) fn duration(&self) -> f32 {
        self.mono_samples.len() as f32 / self.sample_rate.get() as f32
    }
}

/// Owns the audio output stream and tracks playback state and position in seconds.
pub(crate) struct Playback {
    sink: Option<Player>,
    audio: Option<AudioFile>,
    output: Result<MixerDeviceSink, String>,
    position: f32,
    source_offset: f32,
    playing: bool,
}

impl Playback {
    pub(crate) fn new() -> Self {
        Self {
            sink: None,
            audio: None,
            output: DeviceSinkBuilder::open_default_sink()
                .map(|mut output| {
                    output.log_on_drop(false);
                    output
                })
                .map_err(|error| format!("Audio output is unavailable: {error}")),
            position: 0.0,
            source_offset: 0.0,
            playing: false,
        }
    }

    pub(crate) fn output_error(&self) -> Option<&str> {
        self.output.as_ref().err().map(String::as_str)
    }

    pub(crate) fn audio(&self) -> Option<&AudioFile> {
        self.audio.as_ref()
    }

    pub(crate) fn position(&self) -> f32 {
        self.position
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.playing
    }

    /// Replaces the current audio and resets playback only after decoding succeeds.
    pub(crate) fn load(&mut self, path: &Path) -> Result<(), String> {
        let audio = AudioFile::open(path)?;
        self.stop();
        self.audio = Some(audio);
        Ok(())
    }

    /// Resumes paused audio or starts at the selected position, restarting at the end.
    pub(crate) fn play(&mut self) -> Result<(), String> {
        let audio = self
            .audio
            .as_ref()
            .ok_or("Choose an audio file before starting playback.")?;
        let output = self.output.as_ref().map_err(Clone::clone)?;

        if let Some(sink) = &self.sink
            && !sink.empty()
        {
            sink.play();
            self.playing = true;
            return Ok(());
        }

        let position = if self.position >= audio.duration() {
            0.0
        } else {
            self.position
        };
        let start_frame = (position * audio.sample_rate.get() as f32) as usize;
        let start_offset = start_frame * usize::from(audio.channels.get());
        let sink = Player::connect_new(output.mixer());
        sink.append(SamplesBuffer::new(
            audio.channels,
            audio.sample_rate,
            audio.samples[start_offset..].to_vec(),
        ));
        self.position = start_frame as f32 / audio.sample_rate.get() as f32;
        self.source_offset = self.position;
        self.sink = Some(sink);
        self.playing = true;
        Ok(())
    }

    /// Pauses output and records the sink's latest playback position.
    pub(crate) fn pause(&mut self) {
        if let Some(sink) = &self.sink {
            sink.pause();
        }
        self.update();
        self.playing = false;
    }

    /// Discards queued audio and resets the position without unloading the file.
    pub(crate) fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.playing = false;
        self.position = 0.0;
        self.source_offset = 0.0;
    }

    /// Seeks in seconds, preserving playback unless the destination is the file's end.
    pub(crate) fn seek(&mut self, position: f32) -> Result<(), String> {
        let duration = self
            .audio
            .as_ref()
            .ok_or("Choose an audio file before seeking.")?
            .duration();
        if !position.is_finite() || !(0.0..=duration).contains(&position) {
            return Err("The seek position must be within the audio file.".into());
        }
        let resume = self.playing;
        self.stop();
        self.position = position;
        // Seeking to the end stops there; only an explicit Play restarts the file.
        if resume && position < duration {
            self.play()?;
        }
        Ok(())
    }

    /// Synchronizes position and completion state with the audio sink.
    pub(crate) fn update(&mut self) {
        if !self.playing {
            return;
        }
        if let (Some(sink), Some(audio)) = (&self.sink, &self.audio) {
            if sink.empty() {
                self.position = audio.duration();
                self.playing = false;
            } else {
                self.position =
                    (self.source_offset + sink.get_pos().as_secs_f32()).min(audio.duration());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, thread, time::Duration};

    use super::*;

    fn wav(samples: &[i16]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + samples.len() as u32 * 2).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&48_000_u32.to_le_bytes());
        bytes.extend_from_slice(&192_000_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(samples.len() as u32 * 2).to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    fn playback_without_device(audio: AudioFile) -> Playback {
        Playback {
            sink: None,
            audio: Some(audio),
            output: Err("Test audio output is unavailable.".into()),
            position: 0.0,
            source_offset: 0.0,
            playing: false,
        }
    }

    #[test]
    fn decodes_stereo_wav_to_mono_frames() {
        let audio = AudioFile::decode(
            Path::new("test.wav"),
            Cursor::new(wav(&[16_384, 0, -16_384, 0])),
        )
        .unwrap();
        assert_eq!(audio.channels.get(), 2);
        assert_eq!(audio.sample_rate(), 48_000);
        assert_eq!(audio.mono_samples(), &[0.25, -0.25]);
        assert!((audio.duration() - 2.0 / 48_000.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rejects_invalid_and_empty_audio() {
        assert!(AudioFile::decode(Path::new("invalid.wav"), Cursor::new(vec![0_u8; 64])).is_err());
        assert!(AudioFile::decode(Path::new("empty.wav"), Cursor::new(wav(&[]))).is_err());
    }

    #[test]
    fn seeking_to_end_does_not_restart_playback() {
        let audio = AudioFile::decode(Path::new("test.wav"), Cursor::new(wav(&[0; 96]))).unwrap();
        let end = audio.duration();
        let mut playback = playback_without_device(audio);
        playback.playing = true;
        playback.seek(end).unwrap();
        assert_eq!(playback.position(), end);
        assert!(!playback.is_playing());
    }

    #[test]
    fn stop_resets_position_and_invalid_seeks_are_errors() {
        let audio = AudioFile::decode(Path::new("test.wav"), Cursor::new(wav(&[0; 96]))).unwrap();
        let mut playback = playback_without_device(audio);
        playback.seek(0.0005).unwrap();
        assert_eq!(playback.position(), 0.0005);
        assert!(playback.seek(f32::NAN).is_err());
        assert!(playback.seek(-1.0).is_err());
        assert!(playback.seek(1.0).is_err());
        assert!(playback.play().is_err());
        playback.stop();
        assert_eq!(playback.position(), 0.0);
        assert!(!playback.is_playing());
    }

    #[test]
    #[ignore = "requires an audio output device"]
    fn playback_with_default_output() {
        let mut playback = Playback::new();
        assert!(
            playback.output_error().is_none(),
            "{:?}",
            playback.output_error()
        );
        playback.audio = Some(
            AudioFile::decode(
                Path::new("silent.wav"),
                Cursor::new(wav(&vec![0; 48_000 * 2 * 3])),
            )
            .unwrap(),
        );

        playback.play().unwrap();
        thread::sleep(Duration::from_millis(200));
        playback.update();
        assert!(playback.position() > 0.0);
        assert!(playback.is_playing());

        playback.pause();
        let paused_position = playback.position();
        thread::sleep(Duration::from_millis(100));
        playback.update();
        assert_eq!(playback.position(), paused_position);
        assert!(!playback.is_playing());

        playback.play().unwrap();
        thread::sleep(Duration::from_millis(200));
        playback.update();
        assert!(playback.position() > paused_position);

        playback.seek(1.0).unwrap();
        thread::sleep(Duration::from_millis(200));
        playback.update();
        assert!(playback.position() > 1.0);
        assert!(playback.position() < 2.0);

        playback.seek(3.0).unwrap();
        assert!(!playback.is_playing());
        assert_eq!(playback.position(), 3.0);
        playback.play().unwrap();
        assert_eq!(playback.position(), 0.0);
        playback.stop();
        assert_eq!(playback.position(), 0.0);
        assert!(!playback.is_playing());
    }
}
