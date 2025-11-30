//! API specification importers.
//!
//! This module contains importers for converting API specifications
//! to CCOS capabilities:
//! - OpenAPI/Swagger specs
//! - GraphQL schemas
//! - Generic HTTP API wrapping

pub mod graphql_importer;
pub mod http_wrapper;
pub mod openapi_importer;

// Re-export commonly used types
pub use graphql_importer::{GraphQLImporter, GraphQLOperation, GraphQLSchema};
pub use http_wrapper::{HTTPAPIInfo, HTTPEndpoint, HTTPWrapper};
pub use openapi_importer::{OpenAPIImporter, OpenAPIOperation, OpenAPIInfo};
