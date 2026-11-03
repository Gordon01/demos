# Audio visualiser

An egui application that plays local WAV, MP3, FLAC, and Ogg files and visualizes
their frequency content. On macOS, it can also visualize the system output mix.

## System-audio capture

Select **System output** in the settings window and choose **Start capture**.
Native system-output capture uses Core Audio taps and requires macOS 14.2 or
newer plus **Screen & System Audio Recording** permission. Grant permission in
**System Settings → Privacy & Security → Screen & System Audio Recording**.
The packaged macOS application includes the required
`NSAudioCaptureUsageDescription` entry. Launch it with `nix run .#visualiser`;
direct `cargo run` builds can still play files but are not bundled for system
audio permission prompts.

System capture is currently macOS-only; local-file playback remains available
on Linux and Windows.
