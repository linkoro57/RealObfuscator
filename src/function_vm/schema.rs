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
    RecipeMask,
}

impl Field {
    pub const COUNT: usize = Self::ALL.len();
    pub const ALL: [Self; 13] = [
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
        Self::RecipeMask,
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
            Self::RecipeMask => "__RECIPE_MASK__",
        }
    }
}

/// Luau table slots shared by all prototypes, either dense or sparse per build.
pub(super) struct Schema([usize; Field::COUNT]);

impl Schema {
    pub fn sparse(rng: &mut Rng) -> Self {
        let mut used = std::collections::BTreeSet::new();
        Self(std::array::from_fn(|_| loop {
            let slot = rng.range(1024, 1 << 24) as usize;
            if used.insert(slot) {
                break slot;
            }
        }))
    }

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
    fn sparse_slots_are_unique_reproducible_and_diversified() {
        let mut layouts = BTreeSet::new();
        for seed in 0..64 {
            let schema = Schema::sparse(&mut Rng::new(seed));
            let same = Schema::sparse(&mut Rng::new(seed));
            assert_eq!(schema.0, same.0);
            assert_eq!(
                schema.0.into_iter().collect::<BTreeSet<_>>().len(),
                Field::COUNT
            );
            assert!(schema.0.iter().all(|slot| (1024..1 << 24).contains(slot)));
            layouts.insert(schema.0);
            let expanded = schema.apply(format!(
                "{}\n{}",
                include_str!("runtime.luau"),
                include_str!("invocation.luau")
            ));
            for field in Field::ALL {
                assert!(!expanded.contains(field.placeholder()));
            }
        }
        assert_eq!(layouts.len(), 64);
    }

    #[test]
    fn every_runtime_field_has_a_distinct_one_based_slot() {
        let template = format!(
            "{}\n{}",
            include_str!("runtime.luau"),
            include_str!("invocation.luau")
        );
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
            let expanded = schema.apply(template.clone());
            for field in Field::ALL {
                assert!(!expanded.contains(field.placeholder()));
            }
        }
    }
}
