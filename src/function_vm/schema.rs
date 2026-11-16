//! Shared contract between prototype serialization and interpreter generation.
use crate::rng::Rng;

#[derive(Clone, Copy, Debug)]
pub(super) enum Field {
    Payload,
    Seed,
    Step,
    Salt,
    Layout,
    Entry,
    InstructionCount,
    Constants,
    ConstantValues,
    ConstantKnown,
    Records,
    Offsets,
}

impl Field {
    pub const COUNT: usize = Self::ALL.len();
    pub const ALL: [Self; 12] = [
        Self::Payload,
        Self::Seed,
        Self::Step,
        Self::Salt,
        Self::Layout,
        Self::Entry,
        Self::InstructionCount,
        Self::Constants,
        Self::ConstantValues,
        Self::ConstantKnown,
        Self::Records,
        Self::Offsets,
    ];

    pub fn placeholder(self) -> &'static str {
        match self {
            Self::Payload => "__PAYLOAD__",
            Self::Seed => "__SEED__",
            Self::Step => "__STEP__",
            Self::Salt => "__SALT__",
            Self::Layout => "__LAYOUT__",
            Self::Entry => "__ENTRY__",
            Self::InstructionCount => "__INSTRUCTION_COUNT__",
            Self::Constants => "__CONSTANTS__",
            Self::ConstantValues => "__CONSTANT_VALUES__",
            Self::ConstantKnown => "__CONSTANT_KNOWN__",
            Self::Records => "__RECORDS__",
            Self::Offsets => "__OFFSETS__",
        }
    }
}

/// One-based Luau table slots, shuffled once per build and shared by all prototypes.
pub(super) struct Schema([usize; Field::COUNT]);

impl Schema {
    pub fn new(rng: &mut Rng) -> Self {
        let mut slots = std::array::from_fn(|index| index + 1);
        rng.shuffle(&mut slots);
        Self(slots)
    }

    pub fn slot(&self, field: Field) -> usize {
        self.0[field as usize]
    }

    pub fn apply(&self, mut template: String) -> String {
        for field in Field::ALL {
            template = template.replace(field.placeholder(), &self.slot(field).to_string());
        }
        template
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_runtime_field_has_a_distinct_one_based_slot() {
        let template = include_str!("runtime.luau");
        for field in Field::ALL {
            assert!(
                template.contains(field.placeholder()),
                "{field:?} is unused"
            );
        }
        for seed in 0..64 {
            let schema = Schema::new(&mut Rng::new(seed));
            let slots: BTreeSet<_> = Field::ALL.map(|field| schema.slot(field)).into();
            assert_eq!(slots, (1..=Field::COUNT).collect());
            let expanded = schema.apply(template.to_owned());
            for field in Field::ALL {
                assert!(!expanded.contains(field.placeholder()));
            }
        }
    }
}
