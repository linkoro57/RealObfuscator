//! Layered Luau transformations with deterministic builds and coverage reports.
//!
//! Configuration and source are validated before transformation, and generated
//! artifacts are parsed again before they are returned to the caller.
#![forbid(unsafe_code)]

mod codec;
pub mod config;
mod constants;
mod control_flow;
mod function_vm;
pub mod lexer;
mod names;
pub mod obfuscator;
pub mod rng;
mod syntax;

pub use config::{
    DeadCodeDensity, ObfuscationConfig, ProtectionLevel, StringDecodeMode, VmHardening,
};
pub use obfuscator::{
    obfuscate, ObfuscationError, ObfuscationOutput, ObfuscationReport, Obfuscator,
};
