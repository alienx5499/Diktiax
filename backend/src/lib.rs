//! Library surface shared by the server binary and integration tests.
#![recursion_limit = "256"]
pub mod actions;
pub mod agent;
pub mod api;
pub mod config;
pub mod db;
pub mod domain;
pub mod mcp;
pub mod tools;
