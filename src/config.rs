use crate::rng::Rng;
use std::str::FromStr;

/// When protected string literals are decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringDecodeMode {
    Lazy,
    Eager,
}

impl StringDecodeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lazy => "lazy",
            Self::Eager => "eager",
        }
    }
}

impl FromStr for StringDecodeMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "lazy" => Ok(Self::Lazy),
            "eager" => Ok(Self::Eager),
            _ => Err("string_decode_mode must be lazy or eager".into()),
        }
    }
}

/// Opcode diversification and plaintext constant lifetime in the register VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmHardening {
    Basic,
    ClientMax,
    Max,
}

impl VmHardening {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::ClientMax => "client-max",
            Self::Max => "max",
        }
    }

    pub(crate) fn opcode_aliases(self) -> usize {
        match self {
            Self::Basic => 2,
            Self::ClientMax => 3,
            Self::Max => 4,
        }
    }
}

impl FromStr for VmHardening {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "basic" => Ok(Self::Basic),
            "client-max" => Ok(Self::ClientMax),
            "max" => Ok(Self::Max),
            _ => Err("unknown VM hardening profile".into()),
        }
    }
}

/// Parsed once at the pipeline boundary, after validating the public configuration.
pub(crate) struct ValidatedOptions {
    pub decode_mode: StringDecodeMode,
    pub hardening: VmHardening,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectionLevel {
    Minimal = 1,
    Standard = 2,
    Maximum = 3,
    Paranoid = 4,
}

impl TryFrom<u8> for ProtectionLevel {
    type Error = String;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Minimal),
            2 => Ok(Self::Standard),
            3 => Ok(Self::Maximum),
            4 => Ok(Self::Paranoid),
            _ => Err("level must be between 1 and 4".into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeadCodeDensity {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug)]
pub struct ObfuscationConfig {
    pub level: Option<ProtectionLevel>,
    pub rename_identifiers: bool,
    pub encrypt_strings: bool,
    /// Fragment long source literals before encoding; bounded to eight pieces.
    pub split_strings: bool,
    pub cache_strings: bool,
    pub obfuscate_numbers: bool,
    pub control_flow_flatten: bool,
    /// Legacy arithmetic noise; disabled by presets because it is readily simplified.
    pub opaque_predicates: bool,
    pub inject_dead_code: bool,
    pub table_indirection: bool,
    pub anti_tamper: bool,
    pub virtualize: bool,
    pub virtualize_chunk: bool,
    /// Emit VM tables as independently shuffled data columns, rebuilt at startup.
    pub vm_scatter_data: bool,
    /// Bind compiled string literals to their prototype's encoding context.
    pub vm_bind_strings: bool,
    /// Reconstruct bound literals through shuffled VM instruction recipes.
    pub vm_literal_programs: bool,
    /// Reject runtime string references left outside compiled VM constants.
    pub require_bound_strings: bool,
    pub require_function_vm: bool,
    pub require_chunk_vm: bool,
    pub name_min_length: usize,
    pub name_max_length: usize,
    /// `lazy` or `eager`; prefer `with_string_decode_mode` in new code.
    pub string_decode_mode: String,
    pub mba_depth: u8,
    pub skip_trivial_numbers: bool,
    pub min_blocks_for_cff: usize,
    pub dead_code_density: DeadCodeDensity,
    pub fake_function_count: usize,
    pub wrap_in_iife: bool,
    /// `basic`, `client-max` or `max`; prefer `with_vm_hardening` in new code.
    pub vm_hardening: String,
    pub vm_pack_bytecode: bool,
    pub vm_pack_constants: bool,
    pub vm_predecode_bytecode: bool,
    pub seed: Option<u64>,
    pub minify: bool,
    pub strip_comments: bool,
    pub strip_types: bool,
    pub build_id: String,
    pub(crate) rng: Rng,
}

impl Default for ObfuscationConfig {
    fn default() -> Self {
        let mut random = Rng::random();
        let seed = random.next_u64();
        let build_id = format!("{:016x}", random.next_u64());
        Self {
            level: None,
            rename_identifiers: true,
            encrypt_strings: true,
            split_strings: false,
            cache_strings: true,
            obfuscate_numbers: true,
            control_flow_flatten: true,
            opaque_predicates: false,
            inject_dead_code: true,
            table_indirection: true,
            anti_tamper: true,
            virtualize: false,
            virtualize_chunk: true,
            vm_scatter_data: false,
            vm_bind_strings: false,
            vm_literal_programs: false,
            require_bound_strings: false,
            require_function_vm: false,
            require_chunk_vm: false,
            name_min_length: 8,
            name_max_length: 14,
            string_decode_mode: "lazy".into(),
            mba_depth: 2,
            skip_trivial_numbers: true,
            min_blocks_for_cff: 3,
            dead_code_density: DeadCodeDensity::Medium,
            fake_function_count: 5,
            wrap_in_iife: false,
            vm_hardening: "basic".into(),
            vm_pack_bytecode: false,
            vm_pack_constants: false,
            vm_predecode_bytecode: false,
            seed: None,
            minify: true,
            strip_comments: true,
            strip_types: true,
            build_id,
            rng: Rng::new(seed),
        }
    }
}

impl ObfuscationConfig {
    /// Strict VM coverage with prototype-bound literals. This increases extraction
    /// work, but does not establish confidentiality against a client-side analyst.
    pub fn hardened() -> Self {
        let mut config = Self::default().with_level(ProtectionLevel::Paranoid);
        config.vm_bind_strings = true;
        config.vm_literal_programs = true;
        config.require_bound_strings = true;
        config.require_function_vm = true;
        config.require_chunk_vm = true;
        config
    }

    pub fn with_string_decode_mode(mut self, mode: StringDecodeMode) -> Self {
        self.string_decode_mode = mode.as_str().into();
        self
    }

    pub fn with_vm_hardening(mut self, hardening: VmHardening) -> Self {
        self.vm_hardening = hardening.as_str().into();
        self
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self.build_id = "development".into();
        self.rng = Rng::new(seed);
        self
    }
    pub fn with_level(mut self, level: ProtectionLevel) -> Self {
        let n = level as u8;
        self.level = Some(level);
        self.rename_identifiers = n >= 1;
        self.encrypt_strings = n >= 1;
        self.split_strings = n >= 3;
        self.obfuscate_numbers = n >= 1;
        self.control_flow_flatten = n >= 2;
        self.opaque_predicates = false;
        self.inject_dead_code = n >= 2;
        self.table_indirection = n >= 3;
        self.anti_tamper = n >= 3;
        self.virtualize = n >= 4;
        self.vm_scatter_data = n >= 4;
        self.cache_strings = n < 4;
        if self.virtualize {
            self.vm_hardening = "max".into();
            self.vm_pack_bytecode = true;
            self.vm_pack_constants = true;
        }
        self
    }
    pub fn lightweight() -> Self {
        let mut c = Self::default().with_level(ProtectionLevel::Minimal);
        c.obfuscate_numbers = false;
        c.string_decode_mode = "eager".into();
        c
    }
    pub fn validate(&self) -> Result<(), String> {
        self.validated_options().map(|_| ())
    }

    pub(crate) fn validated_options(&self) -> Result<ValidatedOptions, String> {
        if self.vm_scatter_data && !self.virtualize {
            return Err("vm_scatter_data requires VM mode".into());
        }
        if self.vm_bind_strings && !self.virtualize {
            return Err("vm_bind_strings requires VM mode".into());
        }
        if self.vm_literal_programs && (!self.vm_bind_strings || !self.anti_tamper) {
            return Err("vm_literal_programs requires bound strings and integrity checks".into());
        }
        if self.require_bound_strings && (!self.vm_bind_strings || !self.virtualize_chunk) {
            return Err(
                "require_bound_strings requires prototype-bound strings and chunk compilation"
                    .into(),
            );
        }
        if self.require_bound_strings && !self.anti_tamper {
            return Err("require_bound_strings requires integrity checks".into());
        }
        if self.require_chunk_vm && (!self.virtualize || !self.virtualize_chunk) {
            return Err("require_chunk_vm requires VM mode with chunk compilation enabled".into());
        }
        if self.require_function_vm && !self.virtualize {
            return Err("require_function_vm requires VM mode".into());
        }
        if self.name_min_length == 0 {
            return Err("name_min_length must be at least 1".into());
        }
        if self.name_max_length < self.name_min_length {
            return Err("name_max_length must be >= name_min_length".into());
        }
        if self.name_max_length > 128 {
            return Err("name_max_length must be <= 128".into());
        }
        if !(2..=128).contains(&self.min_blocks_for_cff) {
            return Err("min_blocks_for_cff must be between 2 and 128".into());
        }
        if self.fake_function_count > 64 {
            return Err("fake_function_count must be <= 64".into());
        }
        if self.wrap_in_iife && !self.strip_types {
            return Err(
                "wrap_in_iife requires strip_types (exported types must remain at module scope)"
                    .into(),
            );
        }
        if !(1..=3).contains(&self.mba_depth) {
            return Err("mba_depth must be between 1 and 3".into());
        }
        let decode_mode = self.string_decode_mode.parse()?;
        let hardening = self.vm_hardening.parse()?;
        if self.build_id.contains(['\r', '\n']) {
            return Err("build_id must not contain newlines".into());
        }
        Ok(ValidatedOptions {
            decode_mode,
            hardening,
        })
    }
}
