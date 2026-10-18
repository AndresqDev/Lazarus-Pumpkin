//! Gameplay handlers — mirror of the original `me.qiooip.lazarus.handlers`
//! package. Each submodule groups one original handler package; the event
//! adapters that span several packages live in `crate::listeners`.

pub mod block;
pub mod chat;
pub mod death;
pub mod elevator;
pub mod kitmap;
pub mod leaderboard;
pub mod limiter;
pub mod logger;
pub mod portal;
pub mod rank;
pub mod restore;
pub mod salvage;
pub mod settings;
pub mod timer;
