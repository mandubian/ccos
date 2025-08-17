use crate::runtime::error::RuntimeError;

/// Errors that can occur during RTFS bridge operations
#[derive(Debug, thiserror::Error)]
pub enum RtfsBridgeError {
    #[error("Invalid CCOS function call: {message}")]
    InvalidCcosFunctionCall { message: String },
    
    #[error("Missing required field '{field}' in CCOS object")]
    MissingRequiredField { field: String },
    
    #[error("Invalid field type for '{field}': expected {expected}, got {actual}")]
    InvalidFieldType { field: String, expected: String, actual: String },
    
    #[error("Invalid CCOS object format: {message}")]
    InvalidObjectFormat { message: String },
    
    #[error("Unsupported CCOS object type: {object_type}")]
    UnsupportedObjectType { object_type: String },
    
    #[error("Validation failed: {message}")]
    ValidationFailed { message: String },
    
    #[error("Conversion failed: {message}")]
    ConversionFailed { message: String },
}

impl From<RtfsBridgeError> for RuntimeError {
    fn from(err: RtfsBridgeError) -> Self {
        RuntimeError::Generic(err.to_string())
    }
}
