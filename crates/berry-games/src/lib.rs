//! Game catalogue and per-game library data (settings, library membership, launch options).
//!
//! This crate carries no user-visible text: anything shown in the UI is exposed as a Fluent
//! message id (`label_id`, `description_key`) and translated by the application.

pub mod catalog;
pub mod library;
