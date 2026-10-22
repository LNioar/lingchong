pub mod adapter;
pub mod manager;
pub mod dedup;
pub mod tagging;
pub mod adapters;

pub use adapter::{HotAdapter, FetchContext, FetchError, RateLimit};
pub use manager::{AdapterManager, FetchOutcome};
