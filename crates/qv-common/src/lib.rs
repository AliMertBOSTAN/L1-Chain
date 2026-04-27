//! Shared primitives for the QuantumVault workspace.
//!
//! This crate must remain dependency-free except for `thiserror`, `serde`,
//! `hex`, and `tracing` so that it sits at the very bottom of the dependency
//! graph. Every other `qv-*` crate may depend on `qv-common`.

#![forbid(unsafe_code)]

/// Re-export of the common result alias used throughout QuantumVault.
pub type QvResult<T> = Result<T, QvError>;

/// Top-level error type. Crate-specific errors should convert into this.
#[derive(Debug, thiserror::Error)]
pub enum QvError {
    /// Generic catch-all with an explanatory message.
    #[error("{0}")]
    Generic(String),

    /// Propagated error from an underlying component.
    #[error("{context}: {source}")]
    Wrapped {
        /// Human-readable context.
        context: String,
        /// Source error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

impl QvError {
    /// Construct a generic error from any `Display` value.
    pub fn generic(msg: impl Into<String>) -> Self {
        Self::Generic(msg.into())
    }
}
