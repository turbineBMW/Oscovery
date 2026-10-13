//! Everything in Oscovery that has no widget in it: the catalog model and
//! its index file, the local database and search, the Omarchy palette, the
//! agent runner and the catalog directory it searches, and install plans.

pub mod db;
pub mod error;
pub mod harness;
pub mod index;
pub mod install;
pub mod markdown;
pub mod models;
pub mod omarchy;
pub mod readme;
pub mod retrieve;
pub mod review;
pub mod workdir;
