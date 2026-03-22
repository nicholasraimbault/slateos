/// Security settings page: lock screen timeout, lock-on-suspend, and PIN management.
use iced::widget::{button, column, container, row, slider, text, text_input, toggler};
use iced::{Element, Length};

use slate_common::settings::LockSettings;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum SecurityMsg {
    /// Idle timeout slider changed (value in minutes, 0 = never).
    IdleTimeoutChanged(f32),
    /// Lock-on-suspend toggle.
    LockOnSuspendToggled(bool),
    /// New PIN text input changed.
    NewPinInput(String),
    /// Confirm PIN text input changed.
    ConfirmPinInput(String),
    /// User pressed "Set PIN" / "Change PIN".
    ChangePin,
    /// User pressed "Remove PIN".
    RemovePin,
    /// Result of a PIN operation (success message or error).
    PinResult(Result<String, String>),
}

// ---------------------------------------------------------------------------
// Ephemeral page state (not persisted to settings.toml)
// ---------------------------------------------------------------------------

/// UI state for the PIN management form. Not persisted.
#[derive(Debug, Default, Clone)]
pub struct SecurityPageState {
    pub new_pin: String,
    pub confirm_pin: String,
    pub status_message: Option<Result<String, String>>,
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(
    settings: &mut LockSettings,
    page: &mut SecurityPageState,
    msg: SecurityMsg,
) -> Option<iced::Task<SecurityMsg>> {
    match msg {
        SecurityMsg::IdleTimeoutChanged(minutes) => {
            // Slider is in minutes; store as seconds. 0 = never.
            settings.idle_timeout_secs = (minutes * 60.0) as u64;
            None
        }
        SecurityMsg::LockOnSuspendToggled(v) => {
            settings.lock_on_suspend = v;
            None
        }
        SecurityMsg::NewPinInput(v) => {
            page.new_pin = v;
            None
        }
        SecurityMsg::ConfirmPinInput(v) => {
            page.confirm_pin = v;
            None
        }
        SecurityMsg::ChangePin => {
            if page.new_pin.len() < 4 {
                page.status_message = Some(Err("PIN must be at least 4 digits".to_string()));
                return None;
            }
            if page.new_pin.len() > 8 {
                page.status_message = Some(Err("PIN must be at most 8 digits".to_string()));
                return None;
            }
            if !page.new_pin.chars().all(|c| c.is_ascii_digit()) {
                page.status_message = Some(Err("PIN must contain only digits".to_string()));
                return None;
            }
            if page.new_pin != page.confirm_pin {
                page.status_message = Some(Err("PINs do not match".to_string()));
                return None;
            }

            let pin = page.new_pin.clone();
            page.new_pin.clear();
            page.confirm_pin.clear();

            // Hash and save on a background thread (argon2 is CPU-intensive).
            Some(iced::Task::perform(
                async move { save_pin_credential(&pin).await },
                SecurityMsg::PinResult,
            ))
        }
        SecurityMsg::RemovePin => {
            page.new_pin.clear();
            page.confirm_pin.clear();

            Some(iced::Task::perform(
                async { remove_pin_credential().await },
                SecurityMsg::PinResult,
            ))
        }
        SecurityMsg::PinResult(result) => {
            page.status_message = Some(result);
            None
        }
    }
}

/// Hash and save a PIN credential to ~/.config/slate/lock.toml.
async fn save_pin_credential(pin: &str) -> Result<String, String> {
    let pin = pin.to_string();
    tokio::task::spawn_blocking(move || {
        let home = std::env::var("HOME").map_err(|e| format!("HOME not set: {e}"))?;
        let path = std::path::PathBuf::from(home).join(".config/slate/lock.toml");

        // Import from slate-lock's auth module is not possible here (separate crate).
        // Use argon2 directly — same algorithm as slate-lock/src/auth.rs.
        use argon2::password_hash::rand_core::OsRng;
        use argon2::password_hash::SaltString;
        use argon2::{Argon2, PasswordHasher};

        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(pin.as_bytes(), &salt)
            .map_err(|e| format!("hash failed: {e}"))?
            .to_string();

        // Write lock.toml
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {e}"))?;
        }

        let content = format!("pin_hash = \"{hash}\"\n");
        std::fs::write(&path, &content).map_err(|e| format!("write failed: {e}"))?;

        // Set 0600 permissions on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&path, perms).map_err(|e| format!("chmod failed: {e}"))?;
        }

        Ok("PIN set successfully".to_string())
    })
    .await
    .map_err(|e| format!("task failed: {e}"))?
}

/// Remove the PIN credential file.
async fn remove_pin_credential() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        let home = std::env::var("HOME").map_err(|e| format!("HOME not set: {e}"))?;
        let path = std::path::PathBuf::from(home).join(".config/slate/lock.toml");

        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("remove failed: {e}"))?;
            Ok("PIN removed".to_string())
        } else {
            Ok("No PIN was set".to_string())
        }
    })
    .await
    .map_err(|e| format!("task failed: {e}"))?
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view<'a>(
    settings: &'a LockSettings,
    page: &'a SecurityPageState,
) -> Element<'a, SecurityMsg> {
    // Idle timeout: slider from 0–30 minutes. 0 = never auto-lock.
    let timeout_minutes = settings.idle_timeout_secs as f32 / 60.0;
    let timeout_label = if settings.idle_timeout_secs == 0 {
        "Never".to_string()
    } else {
        format!("{} min", settings.idle_timeout_secs / 60)
    };

    let timeout_row = column![
        text("Auto-lock after idle").size(16),
        row![
            slider(0.0..=30.0, timeout_minutes, SecurityMsg::IdleTimeoutChanged).step(1.0),
            text(timeout_label).size(14),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(4);

    let suspend_toggle = toggler(settings.lock_on_suspend)
        .label("Lock when device suspends")
        .on_toggle(SecurityMsg::LockOnSuspendToggled);

    // PIN management form
    let pin_section = column![
        text("PIN Management").size(20),
        text_input("New PIN (4-8 digits)", &page.new_pin)
            .on_input(SecurityMsg::NewPinInput)
            .secure(true)
            .width(Length::Fixed(250.0)),
        text_input("Confirm PIN", &page.confirm_pin)
            .on_input(SecurityMsg::ConfirmPinInput)
            .secure(true)
            .width(Length::Fixed(250.0)),
        row![
            button(text("Set PIN").size(14))
                .on_press(SecurityMsg::ChangePin)
                .padding(8),
            button(text("Remove PIN").size(14))
                .on_press(SecurityMsg::RemovePin)
                .padding(8),
        ]
        .spacing(8),
    ]
    .spacing(8);

    // Status message
    let status: Element<'_, SecurityMsg> = match &page.status_message {
        Some(Ok(msg)) => text(msg.clone())
            .size(14)
            .color(iced::Color::from_rgb(0.2, 0.8, 0.2))
            .into(),
        Some(Err(msg)) => text(msg.clone())
            .size(14)
            .color(iced::Color::from_rgb(0.9, 0.3, 0.3))
            .into(),
        None => text("").size(14).into(),
    };

    let content = column![
        text("Security").size(24),
        timeout_row,
        suspend_toggle,
        pin_section,
        status,
    ]
    .spacing(16)
    .padding(20)
    .width(Length::Fill);

    container(content).width(Length::Fill).into()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_timeout_conversion() {
        let mut s = LockSettings::default();
        let mut p = SecurityPageState::default();
        update(&mut s, &mut p, SecurityMsg::IdleTimeoutChanged(5.0));
        assert_eq!(s.idle_timeout_secs, 300);
    }

    #[test]
    fn idle_timeout_zero_means_never() {
        let mut s = LockSettings::default();
        let mut p = SecurityPageState::default();
        update(&mut s, &mut p, SecurityMsg::IdleTimeoutChanged(0.0));
        assert_eq!(s.idle_timeout_secs, 0);
    }

    #[test]
    fn lock_on_suspend_toggles() {
        let mut s = LockSettings::default();
        let mut p = SecurityPageState::default();
        assert!(s.lock_on_suspend);
        update(&mut s, &mut p, SecurityMsg::LockOnSuspendToggled(false));
        assert!(!s.lock_on_suspend);
    }

    #[test]
    fn pin_too_short_rejected() {
        let mut s = LockSettings::default();
        let mut p = SecurityPageState {
            new_pin: "12".to_string(),
            confirm_pin: "12".to_string(),
            ..Default::default()
        };
        update(&mut s, &mut p, SecurityMsg::ChangePin);
        assert!(matches!(p.status_message, Some(Err(_))));
    }

    #[test]
    fn pin_mismatch_rejected() {
        let mut s = LockSettings::default();
        let mut p = SecurityPageState {
            new_pin: "1234".to_string(),
            confirm_pin: "5678".to_string(),
            ..Default::default()
        };
        update(&mut s, &mut p, SecurityMsg::ChangePin);
        assert!(matches!(p.status_message, Some(Err(_))));
    }

    #[test]
    fn pin_non_digit_rejected() {
        let mut s = LockSettings::default();
        let mut p = SecurityPageState {
            new_pin: "12ab".to_string(),
            confirm_pin: "12ab".to_string(),
            ..Default::default()
        };
        update(&mut s, &mut p, SecurityMsg::ChangePin);
        assert!(matches!(p.status_message, Some(Err(_))));
    }

    #[test]
    fn pin_input_updates_state() {
        let mut s = LockSettings::default();
        let mut p = SecurityPageState::default();
        update(&mut s, &mut p, SecurityMsg::NewPinInput("5678".to_string()));
        assert_eq!(p.new_pin, "5678");
        update(
            &mut s,
            &mut p,
            SecurityMsg::ConfirmPinInput("5678".to_string()),
        );
        assert_eq!(p.confirm_pin, "5678");
    }
}
