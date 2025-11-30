//! Output formatting for CLI commands

use colored::Colorize;
use serde::Serialize;
use std::fmt::Display;

/// Supported output formats
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable table format (default)
    #[default]
    Table,
    /// JSON format
    Json,
    /// RTFS format
    Rtfs,
    /// Plain text (minimal formatting)
    Plain,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "json" => Ok(OutputFormat::Json),
            "rtfs" => Ok(OutputFormat::Rtfs),
            "plain" => Ok(OutputFormat::Plain),
            _ => Err(format!(
                "Unknown output format '{}'. Valid options: table, json, rtfs, plain",
                s
            )),
        }
    }
}

impl Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Table => write!(f, "table"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Rtfs => write!(f, "rtfs"),
            OutputFormat::Plain => write!(f, "plain"),
        }
    }
}

/// Output formatter for consistent CLI output
pub struct OutputFormatter {
    format: OutputFormat,
}

impl OutputFormatter {
    pub fn new(format: OutputFormat) -> Self {
        Self { format }
    }

    /// Print a success message
    pub fn success(&self, message: &str) {
        match self.format {
            OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::json!({"status": "success", "message": message})
                );
            }
            _ => {
                println!("{} {}", "✓".green(), message);
            }
        }
    }

    /// Print an error message
    pub fn error(&self, message: &str) {
        match self.format {
            OutputFormat::Json => {
                eprintln!(
                    "{}",
                    serde_json::json!({"status": "error", "message": message})
                );
            }
            _ => {
                eprintln!("{} {}", "✗".red(), message);
            }
        }
    }

    /// Print a warning message
    pub fn warning(&self, message: &str) {
        match self.format {
            OutputFormat::Json => {
                eprintln!(
                    "{}",
                    serde_json::json!({"status": "warning", "message": message})
                );
            }
            _ => {
                eprintln!("{} {}", "⚠".yellow(), message);
            }
        }
    }

    /// Print data as JSON
    pub fn json<T: Serialize>(&self, data: &T) {
        match serde_json::to_string_pretty(data) {
            Ok(json) => println!("{}", json),
            Err(e) => self.error(&format!("Failed to serialize to JSON: {}", e)),
        }
    }

    /// Print a simple key-value pair
    pub fn kv(&self, key: &str, value: &str) {
        match self.format {
            OutputFormat::Json => {
                println!("{}", serde_json::json!({key: value}));
            }
            OutputFormat::Table => {
                println!("{}: {}", key.cyan(), value);
            }
            _ => {
                println!("{}: {}", key, value);
            }
        }
    }

    /// Print a table header
    pub fn table_header(&self, columns: &[&str]) {
        if self.format == OutputFormat::Table {
            let header: Vec<_> = columns.iter().map(|c| c.bold().to_string()).collect();
            println!("{}", header.join("  "));
            println!("{}", "-".repeat(columns.iter().map(|c| c.len() + 2).sum()));
        }
    }

    /// Print a table row
    pub fn table_row(&self, values: &[&str]) {
        if self.format == OutputFormat::Table {
            println!("{}", values.join("  "));
        }
    }

    /// Print a section title
    pub fn section(&self, title: &str) {
        match self.format {
            OutputFormat::Table => {
                println!();
                println!("{}", title.bold().underline());
                println!();
            }
            OutputFormat::Plain => {
                println!();
                println!("{}", title);
                println!();
            }
            _ => {}
        }
    }

    /// Print a list item
    pub fn list_item(&self, item: &str) {
        match self.format {
            OutputFormat::Table => {
                println!("  {} {}", "•".cyan(), item);
            }
            _ => {
                println!("  - {}", item);
            }
        }
    }
}
