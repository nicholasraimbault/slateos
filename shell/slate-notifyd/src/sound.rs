//! Notification sound playback via PipeWire.
//!
//! Uses `pw-play` to play a standard freedesktop sound file.
//! Failures are logged at DEBUG level so missing sound files or
//! a missing PipeWire daemon do not surface as errors to callers.

/// Path to the freedesktop notification sound installed by the sound-theme-freedesktop package.
const NOTIFICATION_SOUND: &str = "/usr/share/sounds/freedesktop/stereo/message-new-instant.ogg";

/// Play the default notification sound.
///
/// Spawns `pw-play` as a fire-and-forget subprocess. If the sound file does
/// not exist or `pw-play` cannot be started, the error is logged and silently
/// ignored — audio failures must never interrupt notification delivery.
///
/// Uses `std::process::Command` (not tokio) because this may be called from
/// a zbus handler thread that lacks a tokio runtime context.
pub fn play_notification_sound() {
    if !std::path::Path::new(NOTIFICATION_SOUND).exists() {
        return;
    }
    match std::process::Command::new("pw-play")
        .arg(NOTIFICATION_SOUND)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => {}
        Err(e) => tracing::debug!("notification sound failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_sound_path_is_absolute() {
        assert!(NOTIFICATION_SOUND.starts_with('/'));
    }

    #[test]
    fn play_sound_does_not_panic_when_file_missing() {
        // The sound file almost certainly does not exist in the test environment.
        // The function should return without panicking.
        play_notification_sound();
    }
}
