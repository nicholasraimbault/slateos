//! Notification sound playback via PipeWire.
//!
//! Uses `pw-play` to play a standard freedesktop sound file.
//! Failures are logged at DEBUG level so missing sound files or
//! a missing PipeWire daemon do not surface as errors to callers.

/// Path to the freedesktop notification sound installed by the sound-theme-freedesktop package.
const NOTIFICATION_SOUND: &str = "/usr/share/sounds/freedesktop/stereo/message-new-instant.ogg";

/// Play the default notification sound asynchronously.
///
/// Returns immediately after spawning `pw-play`. If the sound file does not
/// exist or `pw-play` cannot be started, the error is logged and silently
/// ignored — audio failures must never interrupt notification delivery.
pub async fn play_notification_sound() {
    if !tokio::fs::try_exists(NOTIFICATION_SOUND)
        .await
        .unwrap_or(false)
    {
        return;
    }
    match tokio::process::Command::new("pw-play")
        .arg(NOTIFICATION_SOUND)
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

    #[tokio::test]
    async fn play_sound_does_not_panic_when_file_missing() {
        // The sound file almost certainly does not exist in the test environment.
        // The function should return without panicking.
        play_notification_sound().await;
    }
}
