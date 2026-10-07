//! Bootstrap pNet onto this machine from a local `pnet` binary, or from the
//! latest published release when you do not have one.
//!
//! This crate is not a pNet app. It does not register with the fabric, mount
//! a portal page, or install other programs.

pub mod bootstrap;
pub mod release;
pub mod setup;
