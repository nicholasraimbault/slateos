// Error types for the slate-notifyd daemon.
//
// Uses thiserror for structured errors in library-style modules (store,
// grouping, history). The binary main.rs uses anyhow for top-level errors.

// ---------------------------------------------------------------------------
// Store errors
// ---------------------------------------------------------------------------

/// Errors from notification store operations (persistence I/O).
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML serialization error: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("TOML deserialization error: {0}")]
    Deserialize(#[from] toml::de::Error),
}

// ---------------------------------------------------------------------------
// History errors
// ---------------------------------------------------------------------------

/// Errors from history persistence operations.
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML serialization error: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("TOML deserialization error: {0}")]
    Deserialize(#[from] toml::de::Error),

    #[error("failed to determine history directory")]
    NoBaseDir,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_error_display_io() {
        let err = StoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not found",
        ));
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn history_error_display_no_base_dir() {
        let err = HistoryError::NoBaseDir;
        assert!(err.to_string().contains("history directory"));
    }
}
