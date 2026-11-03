You need to implement an audio visualizer. It should be a GUI application that can visualize system audio, for example, the sound from a YouTube video playing in a browser.

As a simpler or secondary option, it can also play and visualize WAV or other audio files.

The GUI needs an output/visualization window and a settings window. Add any audio controls you consider useful.

# Algorithm

The visualization consists of horizontal rows, where each row represents a quantized frequency band, from the lowest frequencies at the bottom to the highest frequencies at the top.

For example, frequency bands could be:

```text
20–40 Hz
40–80 Hz
80–160 Hz
160–320 Hz
...
```

Use logarithmically spaced frequency bands, as this should provide a more useful visualization for human perception. The number and frequency range of the bands should ideally be configurable through the GUI.

The visualization works somewhat like a spectrogram, but **each frequency band moves independently**.

For every audio-analysis window:

1. Run an FFT on the audio samples.
2. Group the FFT bins into the corresponding frequency bands.
3. Calculate the aggregate amplitude of each frequency band by summing the magnitudes of its FFT bins.
4. Normalize the resulting amplitude and use it to determine the visual intensity of that row.
5. Generate a new visual slice for each frequency band.

The visualization behaves like a timeline moving from **right to left**. However, unlike a conventional spectrogram, each row has its own horizontal velocity.

The velocity of a row is determined by the amplitude of its frequency band:

- Higher amplitude → faster movement.
- Lower amplitude → slower movement.
- No significant audio in the band → the row stops moving.

Therefore, different frequency bands can move at different speeds and can be at different horizontal positions at the same time.

The amplitude should also control the brightness/color of the visualization:

- Lower amplitude → darker color.
- Higher amplitude → lighter/brighter color.

Use a flamegraph-like color mapping, where increasing intensity transitions from dark colors to bright/high-intensity colors.

The result should look like a continuously moving visualization of the frequency content of the audio, with each frequency band having its own independently moving history.

# Technical Requirements

- Target platform: macOS on Apple Silicon.
- Language: Rust.
- GUI framework: `egui`.
- Since this is a Rust application using `egui`, keep Windows and Linux compatibility in mind.