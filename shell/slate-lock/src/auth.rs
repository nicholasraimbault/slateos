// Hybrid PAM/PIN authentication for the lock screen.
//
// Authentication strategy:
//   1. On Linux, try PAM first — this covers system passwords, biometric
//      modules, and any PAM stack the distro ships.
//   2. Fall back to a local PIN credential file (`lock.toml`) hashed with
//      Argon2id. This path also serves non-Linux dev builds.
//
// The PIN credential file lives alongside settings.toml and stores a single
// Argon2id PHC hash. File permissions are locked to 0600 on Unix so only the
// owning user can read the hash.

use std::path::Path;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// On-disk representation of the lock credential file (`lock.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockCredential {
    /// Argon2id PHC-format hash of the user's PIN.
    pub pin_hash: String,
}

/// Outcome of an authentication attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthResult {
    /// Credential matched — unlock the screen.
    Success,
    /// A credential exists but the supplied value was wrong.
    WrongCredential,
    /// No credential is configured (no PAM, no PIN file).
    NotConfigured,
}

// ---------------------------------------------------------------------------
// Hashing helpers
// ---------------------------------------------------------------------------

/// Hash a PIN with Argon2id and a random salt.
///
/// Returns the PHC-format string (e.g. `$argon2id$v=19$...`).
/// Each call produces a different hash because a fresh random salt is
/// generated every time.
pub fn hash_pin(pin: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    // Argon2::hash_password only fails if parameters are invalid; the default
    // params are always valid, so this is safe to expect in practice.
    argon2
        .hash_password(pin.as_bytes(), &salt)
        .expect("argon2 default params are always valid")
        .to_string()
}

/// Verify a PIN against a stored Argon2id PHC hash.
pub fn verify_pin(pin: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("failed to parse stored PIN hash: {e}");
            return false;
        }
    };

    Argon2::default()
        .verify_password(pin.as_bytes(), &parsed)
        .is_ok()
}

// ---------------------------------------------------------------------------
// Credential file I/O
// ---------------------------------------------------------------------------

/// Load the PIN hash from a `lock.toml` credential file.
///
/// Returns `None` if the file does not exist or cannot be parsed.
pub fn load_credential(path: &Path) -> Option<String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("no credential file at {}: {e}", path.display());
            return None;
        }
    };

    match toml::from_str::<LockCredential>(&content) {
        Ok(cred) => Some(cred.pin_hash),
        Err(e) => {
            tracing::warn!("malformed credential file at {}: {e}", path.display());
            None
        }
    }
}

/// Persist a PIN hash to `lock.toml`, creating parent directories as needed.
///
/// On Unix the file permissions are set to 0600 so only the owning user can
/// read the hash material.
pub fn save_credential(path: &Path, pin_hash: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let cred = LockCredential {
        pin_hash: pin_hash.to_string(),
    };
    let content = toml::to_string_pretty(&cred)?;
    std::fs::write(path, &content)?;

    // Restrict permissions so only the owner can read the hash.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }

    tracing::info!("saved lock credential to {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Top-level authentication
// ---------------------------------------------------------------------------

/// Authenticate a user for the lock screen.
///
/// Strategy:
///   1. On Linux, attempt PAM authentication first — it handles system
///      passwords, LDAP, biometrics, and anything the PAM stack provides.
///   2. Fall back to the local PIN credential file.
///   3. If neither mechanism has a credential, return `NotConfigured`.
pub fn authenticate_sync(pin: &str, username: &str, credential_path: &Path) -> AuthResult {
    // Try PAM first on Linux. If PAM succeeds we are done; if it returns
    // WrongCredential we still fall through to the PIN file because the user
    // may have entered a PIN rather than a system password.
    #[cfg(target_os = "linux")]
    if let Some(result) = try_pam(pin, username) {
        if result == AuthResult::Success {
            return result;
        }
        // PAM returned WrongCredential — the input might be a PIN instead of
        // the system password, so continue to the PIN file check.
    }

    // Suppress unused-variable warning on non-Linux where try_pam is absent.
    let _ = username;

    // Fall back to the local PIN credential file.
    match load_credential(credential_path) {
        Some(stored_hash) => {
            if verify_pin(pin, &stored_hash) {
                AuthResult::Success
            } else {
                AuthResult::WrongCredential
            }
        }
        None => AuthResult::NotConfigured,
    }
}

// ---------------------------------------------------------------------------
// PAM backend (Linux only)
// ---------------------------------------------------------------------------

/// Attempt authentication via the system PAM stack.
///
/// Returns `None` if PAM is unavailable (e.g. service file missing or PAM
/// library not installed). Returns `Some(Success)` or `Some(WrongCredential)`
/// depending on whether PAM accepted the password.
#[cfg(target_os = "linux")]
pub fn try_pam(password: &str, username: &str) -> Option<AuthResult> {
    use pam::Client;

    // "login" is the standard PAM service that validates local passwords.
    let mut client = match Client::with_password("login") {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("PAM client creation failed (expected on non-PAM systems): {e}");
            return None;
        }
    };

    client
        .conversation_mut()
        .set_credentials(username, password);

    match client.authenticate() {
        Ok(()) => {
            tracing::info!("PAM authentication succeeded for user {username}");
            Some(AuthResult::Success)
        }
        Err(e) => {
            tracing::debug!("PAM authentication failed for user {username}: {e}");
            Some(AuthResult::WrongCredential)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_pin() {
        let hash = hash_pin("1234");
        assert!(verify_pin("1234", &hash));
        assert!(!verify_pin("5678", &hash));
    }

    #[test]
    fn hash_is_different_each_time() {
        let h1 = hash_pin("same_input");
        let h2 = hash_pin("same_input");
        // Random salt ensures distinct hashes even for identical input.
        assert_ne!(h1, h2);
        // Both must still verify against the original input.
        assert!(verify_pin("same_input", &h1));
        assert!(verify_pin("same_input", &h2));
    }

    #[test]
    fn empty_pin_hashes_and_verifies() {
        let hash = hash_pin("");
        assert!(verify_pin("", &hash));
        assert!(!verify_pin("notempty", &hash));
    }

    #[test]
    fn load_credential_from_missing_file() {
        let result = load_credential(Path::new("/nonexistent/lock.toml"));
        assert!(result.is_none());
    }

    #[test]
    fn save_and_load_credential_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock.toml");

        let pin_hash = hash_pin("9876");
        save_credential(&path, &pin_hash).unwrap();

        let loaded = load_credential(&path);
        assert_eq!(loaded, Some(pin_hash));
    }

    #[test]
    fn authenticate_with_pin_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock.toml");

        let pin_hash = hash_pin("4321");
        save_credential(&path, &pin_hash).unwrap();

        assert_eq!(
            authenticate_sync("4321", "testuser", &path),
            AuthResult::Success,
        );
        assert_eq!(
            authenticate_sync("0000", "testuser", &path),
            AuthResult::WrongCredential,
        );
    }

    #[test]
    fn authenticate_not_configured() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does_not_exist.toml");

        assert_eq!(
            authenticate_sync("1234", "testuser", &path),
            AuthResult::NotConfigured,
        );
    }
}
