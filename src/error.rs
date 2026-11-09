// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Error types for card operations.

use crate::card::SlotId;
use std::fmt;

/// Errors returned by card operations (disk attach/detach, configuration, etc.).
#[derive(Debug)]
pub enum CardError {
    /// The card does not support this operation.
    NotSupported,
    /// No card is inserted in the specified slot.
    SlotNotFound(SlotId),
    /// The specified drive was not found.
    DriveNotFound {
        /// Drive category (e.g., "floppy", "hd").
        drive_type: String,
        /// Drive number within its category.
        drive_num: usize,
    },
    /// Error loading or saving a disk image.
    DiskImage(String),
    /// Invalid configuration parameter.
    InvalidParam(String),
    /// Filesystem I/O error.
    Io(std::io::Error),
    /// The specified drive alias was not found.
    AliasNotFound(String),
    /// Duplicate drive alias name.
    DuplicateAlias(String),
    /// Generic error with a descriptive message.
    Other(String),
}

impl fmt::Display for CardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CardError::NotSupported => write!(f, "operation not supported by this card"),
            CardError::SlotNotFound(slot) => write!(f, "no card in {slot}"),
            CardError::DriveNotFound {
                drive_type,
                drive_num,
            } => write!(f, "drive {drive_type}:{drive_num} not found"),
            CardError::DiskImage(msg) => write!(f, "disk image error: {msg}"),
            CardError::InvalidParam(msg) => write!(f, "invalid parameter: {msg}"),
            CardError::Io(e) => write!(f, "I/O error: {e}"),
            CardError::AliasNotFound(name) => write!(f, "drive alias '{name}' not found"),
            CardError::DuplicateAlias(name) => {
                write!(f, "drive alias '{name}' is already registered")
            }
            CardError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for CardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CardError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<String> for CardError {
    fn from(s: String) -> Self {
        CardError::Other(s)
    }
}

impl From<&str> for CardError {
    fn from(s: &str) -> Self {
        CardError::Other(s.to_string())
    }
}

impl From<std::io::Error> for CardError {
    fn from(e: std::io::Error) -> Self {
        CardError::Io(e)
    }
}

/// Result type alias for card operations.
pub type CardResult<T> = Result<T, CardError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn display_messages() {
        assert_eq!(
            CardError::NotSupported.to_string(),
            "operation not supported by this card"
        );
        assert_eq!(
            CardError::SlotNotFound(SlotId(5)).to_string(),
            "no card in slot:5"
        );
        assert_eq!(
            CardError::DriveNotFound {
                drive_type: "floppy".into(),
                drive_num: 2,
            }
            .to_string(),
            "drive floppy:2 not found"
        );
        assert_eq!(
            CardError::DiskImage("bad sector".into()).to_string(),
            "disk image error: bad sector"
        );
        assert_eq!(
            CardError::InvalidParam("baud".into()).to_string(),
            "invalid parameter: baud"
        );
        assert_eq!(
            CardError::AliasNotFound("a:".into()).to_string(),
            "drive alias 'a:' not found"
        );
        assert_eq!(
            CardError::DuplicateAlias("a:".into()).to_string(),
            "drive alias 'a:' is already registered"
        );
        assert_eq!(CardError::Other("boom".into()).to_string(), "boom");
    }

    #[test]
    fn io_variant_exposes_source() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err = CardError::Io(io);
        assert!(err.to_string().starts_with("I/O error: "));
        assert!(err.source().is_some());
    }

    #[test]
    fn non_io_variants_have_no_source() {
        assert!(CardError::NotSupported.source().is_none());
        assert!(CardError::Other("x".into()).source().is_none());
    }

    #[test]
    fn from_conversions() {
        assert!(matches!(
            CardError::from("oops".to_string()),
            CardError::Other(s) if s == "oops"
        ));
        assert!(matches!(
            CardError::from("oops"),
            CardError::Other(s) if s == "oops"
        ));
        let io = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(matches!(CardError::from(io), CardError::Io(_)));
    }

    #[test]
    fn card_result_is_usable() {
        fn ok() -> CardResult<u8> {
            Ok(7)
        }
        fn err() -> CardResult<u8> {
            Err(CardError::NotSupported)
        }
        assert_eq!(ok().unwrap(), 7);
        assert!(err().is_err());
    }
}
