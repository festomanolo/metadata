//! Core types for `metadata`: versioned schema, time model, hashing, errors.
pub mod error;
pub mod hash;
pub mod schema;
pub mod time;

pub use error::{MetaError, Result};
pub use schema::*;
pub use time::TimeValue;
