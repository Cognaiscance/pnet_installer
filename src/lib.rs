//! Installer agent: catalog + desired apps + status. Notify only for catalog apps.
//! `bootstrap` installs pNet + this agent from a local binary directory.

pub mod bootstrap;
pub mod card;
pub mod catalog;
pub mod fabric;
pub mod fetch;
pub mod proto;
pub mod setup;
pub mod sources;
pub mod state;
pub mod sync;
pub mod web;
