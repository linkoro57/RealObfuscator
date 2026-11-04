pub mod config;
pub mod lexer;
pub mod obfuscator;
pub mod rng;

pub use config::{DeadCodeDensity, ObfuscationConfig, ProtectionLevel};
pub use obfuscator::{obfuscate, ObfuscationError, Obfuscator};

