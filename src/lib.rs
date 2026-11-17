//! Free, open-source Lua/Luau source obfuscation targeting Roblox and Luau.
//!
//! RealObfuscator provides a Rust library and a local command-line tool. Input Lua
//! must be compatible with Luau; generated code targets the Luau runtime.
//!
//! Configuration and source are validated before transformation, and generated
//! artifacts are parsed again before they are returned to the caller.
//!
//! ```
//! use realobfuscator::{ObfuscationConfig, Obfuscator};
//!
//! let output = Obfuscator::new(ObfuscationConfig::lightweight())
//!     .obfuscate_with_report("local greeting='hello';return greeting")
//!     .expect("valid Luau source");
//! assert_eq!(output.report.encrypted_strings, 1);
//! ```
//!
//! See the [project README](https://github.com/linkoro57/RealObfuscator#readme)
//! for installation, CLI profiles, compatibility limits and benchmark reports.
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
mod source;
mod syntax;
pub mod watermark;
pub use source::decode_source_bytes;
pub use watermark::{detect_watermark, WatermarkDetection};

pub use config::{
    DeadCodeDensity, ObfuscationConfig, ProtectionLevel, StringDecodeMode, VmHardening,
};
pub use obfuscator::{
    obfuscate, ObfuscationError, ObfuscationOutput, ObfuscationReport, Obfuscator,
};
