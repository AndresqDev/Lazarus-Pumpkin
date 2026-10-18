//! Portal handlers — mirror of the original `me.qiooip.lazarus.handlers.portal`
//! package.
//!
//! * [`end_portal`] — `EndPortalHandler`: the `/endportal` selection check.
//! * [`nether_trap`] — `NetherPortalTrapHandler`: client-side air around a
//!   clicked nether portal block so trapped players can use commands again.
//! * [`portal`] — `PortalHandler`: nether coordinate translation plus the
//!   spawn/exit redirections applied on portal travel.

pub mod end_portal;
pub mod nether_trap;
pub mod portal;

pub use nether_trap::NetherPortalTrapListener;
pub use portal::PortalChangeWorldListener;
