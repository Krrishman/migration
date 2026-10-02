//! Security primitives: safe paths, exclusion rules, encryption, redaction
//! and elevation checks. Nothing here performs privilege escalation or
//! accesses protected secrets.

pub mod encryption;
pub mod exclusions;
pub mod redaction;
pub mod safe_path;
