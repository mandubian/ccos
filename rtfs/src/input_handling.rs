// Shared input handling for RTFS compiler binaries
// Provides consistent input source handling across rtfs_compiler and rtfs_repl

use clap::ValueEnum;
use std::fs;
use std::io::{self, BufRead};
use std::path::PathBuf;

/// Input source types supported by RTFS binaries
#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum InputSource {
    /// Interactive REPL mode (rtfs_repl only)
    Interactive,
    /// Execute a string directly
    String,
    /// Execute a file
    File,
    /// Read from stdin pipe
    Pipe,
}

/// Configuration for input handling
#[derive(Debug, Clone)]
pub struct InputConfig {
    pub source: InputSource,
    pub file_path: Option<PathBuf>,
    pub string_content: Option<String>,
    pub verbose: bool,
}

impl InputConfig {
    /// Create a new input config for file input
    pub fn from_file(file_path: PathBuf, verbose: bool) -> Self {
        Self {
            source: InputSource::File,
            file_path: Some(file_path),
            string_content: None,
            verbose,
        }
    }

    /// Create a new input config for string input
    pub fn from_string(content: String, verbose: bool) -> Self {
        Self {
            source: InputSource::String,
            file_path: None,
            string_content: Some(content),
            verbose,
        }
    }

    /// Create a new input config for pipe input
    pub fn from_pipe(verbose: bool) -> Self {
        Self {
            source: InputSource::Pipe,
            file_path: None,
            string_content: None,
            verbose,
        }
    }

    /// Create a new input config for interactive mode
    pub fn interactive(verbose: bool) -> Self {
        Self {
            source: InputSource::Interactive,
            file_path: None,
            string_content: None,
            verbose,
        }
    }
}

/// Result of reading input content
#[derive(Debug)]
pub struct InputContent {
    pub content: String,
    pub source_name: String,
}

/// Read input content based on the configuration
pub fn read_input_content(config: &InputConfig) -> Result<InputContent, InputError> {
    match config.source {
        InputSource::File => {
            let file_path = config
                .file_path
                .as_ref()
                .ok_or_else(|| InputError::MissingFileArgument)?;

            if config.verbose {
                println!("📁 Reading from file: {}", file_path.display());
            }

            let content = fs::read_to_string(file_path).map_err(|e| InputError::FileReadError {
                path: file_path.clone(),
                error: e,
            })?;

            if config.verbose {
                println!("📝 File content ({} bytes):", content.len());
                if content.len() < 500 {
                    println!("{}", content);
                } else {
                    println!("{}...", &content[..500]);
                }
                println!();
            }

            Ok(InputContent {
                content,
                source_name: file_path.to_string_lossy().to_string(),
            })
        }

        InputSource::String => {
            let content = config
                .string_content
                .as_ref()
                .ok_or_else(|| InputError::MissingStringArgument)?
                .clone();

            if config.verbose {
                println!("📝 Executing string input ({} bytes):", content.len());
                println!("{}", content);
                println!();
            }

            Ok(InputContent {
                content,
                source_name: "<string>".to_string(),
            })
        }

        InputSource::Pipe => {
            if config.verbose {
                println!("📥 Reading from stdin pipe");
            }

            let stdin = io::stdin();
            let mut content = String::new();

            for line in stdin.lock().lines() {
                let line = line.map_err(InputError::StdinReadError)?;
                content.push_str(&line);
                content.push('\n');
            }

            if config.verbose {
                println!("📝 Pipe content ({} bytes):", content.len());
                if content.len() < 500 {
                    println!("{}", content);
                } else {
                    println!("{}...", &content[..500]);
                }
                println!();
            }

            Ok(InputContent {
                content,
                source_name: "<stdin>".to_string(),
            })
        }

        InputSource::Interactive => Err(InputError::InteractiveNotSupported),
    }
}

/// Validate input arguments for a given source type
pub fn validate_input_args(
    source: InputSource,
    file_path: &Option<PathBuf>,
    string_content: &Option<String>,
) -> Result<(), InputError> {
    match source {
        InputSource::File => {
            if file_path.is_none() {
                return Err(InputError::MissingFileArgument);
            }
        }
        InputSource::String => {
            if string_content.is_none() {
                return Err(InputError::MissingStringArgument);
            }
        }
        InputSource::Pipe => {
            // No additional arguments needed for pipe
        }
        InputSource::Interactive => {
            // No additional arguments needed for interactive
        }
    }
    Ok(())
}

/// Detect input source based on available arguments and environment
pub fn detect_input_source(
    file_path: &Option<PathBuf>,
    string_content: &Option<String>,
    is_repl: bool,
) -> Result<InputSource, InputError> {
    // Check if stdin has content (for pipe detection)
    // CCOS dependency: atty::is(atty::Stream::Stdin)
    // Simple check for standalone RTFS - assume stdin has content if we can't determine
    let stdin_has_content = true; // Placeholder

    match (
        file_path.is_some(),
        string_content.is_some(),
        stdin_has_content,
        is_repl,
    ) {
        (true, false, false, _) => Ok(InputSource::File),
        (false, true, false, _) => Ok(InputSource::String),
        (false, false, true, _) => Ok(InputSource::Pipe),
        (false, false, false, true) => Ok(InputSource::Interactive),
        (false, false, false, false) => Err(InputError::NoInputSource),
        _ => Err(InputError::MultipleInputSources),
    }
}

/// Errors that can occur during input handling
#[derive(Debug)]
pub enum InputError {
    /// File path required but not provided
    MissingFileArgument,
    /// String content required but not provided
    MissingStringArgument,
    /// Error reading file
    FileReadError {
        path: PathBuf,
        error: std::io::Error,
    },
    /// Error reading from stdin
    StdinReadError(std::io::Error),
    /// Interactive mode not supported in this context
    InteractiveNotSupported,
    /// No input source detected
    NoInputSource,
    /// Multiple input sources provided
    MultipleInputSources,
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputError::MissingFileArgument => {
                write!(
                    f,
                    "❌ Error: --file argument required when using --input file"
                )
            }
            InputError::MissingStringArgument => {
                write!(
                    f,
                    "❌ Error: --string argument required when using --input string"
                )
            }
            InputError::FileReadError { path, error } => {
                write!(f, "❌ Error reading file '{}': {}", path.display(), error)
            }
            InputError::StdinReadError(error) => {
                write!(f, "❌ Error reading from stdin: {}", error)
            }
            InputError::InteractiveNotSupported => {
                write!(
                    f,
                    "❌ Error: Interactive mode not supported in this context"
                )
            }
            InputError::NoInputSource => {
                write!(f, "❌ Error: No input source specified. Use --input file, --input string, or pipe content")
            }
            InputError::MultipleInputSources => {
                write!(f, "❌ Error: Multiple input sources specified. Use only one of: file, string, or pipe")
            }
        }
    }
}

impl std::error::Error for InputError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_config_from_file() {
        let config = InputConfig::from_file(PathBuf::from("test.rtfs"), true);
        assert_eq!(config.source, InputSource::File);
        assert_eq!(config.file_path, Some(PathBuf::from("test.rtfs")));
        assert_eq!(config.string_content, None);
        assert!(config.verbose);
    }

    #[test]
    fn test_input_config_from_string() {
        let config = InputConfig::from_string("(+ 1 2)".to_string(), false);
        assert_eq!(config.source, InputSource::String);
        assert_eq!(config.file_path, None);
        assert_eq!(config.string_content, Some("(+ 1 2)".to_string()));
        assert!(!config.verbose);
    }

    #[test]
    fn test_validate_input_args() {
        // Valid file input
        assert!(
            validate_input_args(InputSource::File, &Some(PathBuf::from("test.rtfs")), &None)
                .is_ok()
        );

        // Invalid file input (missing path)
        assert!(validate_input_args(InputSource::File, &None, &None).is_err());

        // Valid string input
        assert!(
            validate_input_args(InputSource::String, &None, &Some("(+ 1 2)".to_string())).is_ok()
        );

        // Invalid string input (missing content)
        assert!(validate_input_args(InputSource::String, &None, &None).is_err());

        // Valid pipe input
        assert!(validate_input_args(InputSource::Pipe, &None, &None).is_ok());
    }
}
