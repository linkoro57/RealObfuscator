pub mod config;
mod constants;
mod control_flow;
pub mod lexer;
mod names;
pub mod obfuscator;
pub mod rng;
mod syntax;

pub use config::{DeadCodeDensity, ObfuscationConfig, ProtectionLevel};
pub use obfuscator::{
    obfuscate, ObfuscationError, ObfuscationOutput, ObfuscationReport, Obfuscator,
};
