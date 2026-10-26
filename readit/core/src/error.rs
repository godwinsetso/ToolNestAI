use thiserror::Error;

/// Errors carry a user-facing explanation and, where possible, a recovery hint.
#[derive(Debug, Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum ReaditError {
    #[error("This file type is not supported: {detail}")]
    UnsupportedFormat { detail: String },

    #[error("The document is password-protected. Remove the password in the original app and import it again.")]
    Encrypted,

    #[error("The file appears to be damaged or incomplete: {detail}")]
    Corrupt { detail: String },

    #[error("No readable text was found. {detail}")]
    NoText { detail: String },

    #[error("{detail}")]
    ConversionRequired { detail: String },

    #[error("Storage error: {detail}")]
    Storage { detail: String },

    #[error("File error: {detail}")]
    Io { detail: String },

    #[error("Not found: {detail}")]
    NotFound { detail: String },

    #[error("{detail}")]
    Invalid { detail: String },
}

impl From<rusqlite::Error> for ReaditError {
    fn from(e: rusqlite::Error) -> Self {
        ReaditError::Storage { detail: e.to_string() }
    }
}

impl From<std::io::Error> for ReaditError {
    fn from(e: std::io::Error) -> Self {
        ReaditError::Io { detail: e.to_string() }
    }
}

impl From<zip::result::ZipError> for ReaditError {
    fn from(e: zip::result::ZipError) -> Self {
        ReaditError::Corrupt { detail: format!("archive could not be opened ({e})") }
    }
}

impl From<quick_xml::Error> for ReaditError {
    fn from(e: quick_xml::Error) -> Self {
        ReaditError::Corrupt { detail: format!("invalid document XML ({e})") }
    }
}

pub type Result<T> = std::result::Result<T, ReaditError>;
