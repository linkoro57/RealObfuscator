use crate::rng::Rng;

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
    pub obfuscate_numbers: bool,
    pub control_flow_flatten: bool,
    pub opaque_predicates: bool,
    pub inject_dead_code: bool,
    pub table_indirection: bool,
    pub anti_tamper: bool,
    pub virtualize: bool,
    pub name_min_length: usize,
    pub name_max_length: usize,
    pub string_decode_mode: String,
    pub mba_depth: u8,
    pub skip_trivial_numbers: bool,
    pub min_blocks_for_cff: usize,
    pub dead_code_density: DeadCodeDensity,
    pub fake_function_count: usize,
    pub wrap_in_iife: bool,
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
            obfuscate_numbers: true,
            control_flow_flatten: true,
            opaque_predicates: true,
            inject_dead_code: true,
            table_indirection: true,
            anti_tamper: true,
            virtualize: false,
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
        self.obfuscate_numbers = n >= 1;
        self.control_flow_flatten = n >= 2;
        self.opaque_predicates = n >= 2;
        self.inject_dead_code = n >= 2;
        self.table_indirection = n >= 3;
        self.anti_tamper = n >= 3;
        self.virtualize = n >= 4;
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
        if !matches!(self.string_decode_mode.as_str(), "lazy" | "eager") {
            return Err("string_decode_mode must be lazy or eager".into());
        }
        if !matches!(self.vm_hardening.as_str(), "basic" | "client-max" | "max") {
            return Err("unknown VM hardening profile".into());
        }
        Ok(())
    }
}
