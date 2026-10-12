//! Fetchy core: a multi-connection, resumable download engine.

pub mod dash;
pub mod engine;
pub mod extras;
pub mod ftp;
pub mod grab;
pub mod hls;
pub mod limiter;
pub mod manager;
pub mod server;
pub mod types;
pub mod update;
pub mod util;

pub use manager::Manager;
pub use types::*;
pub use update::REPO as UPDATE_REPO;
