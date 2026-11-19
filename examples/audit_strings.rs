//! Static audit of RealObfuscator's public string-record format. Does not execute Lua.
//! This is a source-informed regression probe, not a general-purpose deobfuscator.
#[path = "audit_strings/adapted.rs"]
mod adapted;
#[path = "audit_strings/recipes.rs"]
mod recipes;
#[path = "audit_strings/static_values.rs"]
mod static_values;
use darklua_core::{
    nodes::{Expression, StringExpression, TableEntry, TableExpression},
    process::{DefaultVisitor, NodeProcessor, NodeVisitor},
    Parser,
};
use std::{collections::BTreeSet, error::Error, fmt::Write, fs, path::PathBuf, time::Instant};

#[derive(Default)]
struct Audit {
    candidates: usize,
    rejected: usize,
    recovered: Vec<(u32, Vec<u8>)>,
}

fn integer(expression: &Expression) -> Option<u32> {
    let Expression::Number(number) = expression else {
        return None;
    };
    let value = number.compute_value();
    (value.is_finite() && value.fract() == 0.0 && (0.0..=u32::MAX as f64).contains(&value))
        .then_some(value as u32)
}

fn bytes(expression: &Expression) -> Option<Vec<u8>> {
    match expression {
        Expression::String(value) => Some(value.get_value().to_vec()),
        Expression::Table(table) => table
            .iter_entries()
            .map(|entry| match entry {
                TableEntry::Value(value) => u8::try_from(integer(value)?).ok(),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

impl NodeProcessor for Audit {
    fn process_table_expression(&mut self, table: &mut TableExpression) {
        for entry in table.iter_entries() {
            let TableEntry::Index(entry) = entry else {
                continue;
            };
            let Some(id) = integer(entry.get_key()) else {
                continue;
            };
            let Expression::Table(record) = entry.get_value() else {
                continue;
            };
            let values: Vec<_> = record.iter_entries().collect();
            let [TableEntry::Value(data), TableEntry::Value(seed), TableEntry::Value(step), TableEntry::Value(salt), TableEntry::Value(expected)] =
                values.as_slice()
            else {
                continue;
            };
            let (Some(data), Some(seed), Some(step), Some(salt), Some(expected)) = (
                bytes(data),
                integer(seed),
                integer(step),
                integer(salt),
                integer(expected),
            ) else {
                continue;
            };
            self.candidates += 1;
            // Independently reproduce the emitted runtime's inverse, using only
            // record data present in the artifact. Never call the encoder or Lua.
            let mut state = seed ^ salt;
            let mut hash = 0u64;
            let mut plain = Vec::with_capacity(data.len());
            for (index, cipher) in data.into_iter().enumerate() {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let value = cipher ^ state as u8;
                plain.push(value);
                hash = (hash * 65599 + u64::from(value)) % 2147483647;
                state = state
                    .wrapping_add(step)
                    .wrapping_add(u32::from(cipher))
                    .wrapping_add(index as u32 + 1);
            }
            if hash == u64::from(expected) {
                self.recovered.push((id, plain));
            } else {
                self.rejected += 1;
            }
        }
    }
}

fn extract(source: &str) -> Result<Audit, Box<dyn Error>> {
    stacker::maybe_grow(16 * 1024 * 1024, 32 * 1024 * 1024, || {
        let mut block = Parser::default()
            .parse(source)
            .map_err(|error| error.to_string())?;
        let mut audit = Audit::default();
        DefaultVisitor::visit_block(&mut block, &mut audit);
        Ok(audit)
    })
}

#[derive(Default)]
struct Literals(BTreeSet<Vec<u8>>);
impl NodeProcessor for Literals {
    fn process_string_expression(&mut self, value: &mut StringExpression) {
        self.0.insert(value.get_value().to_vec());
    }
}

fn source_literals(source: &str) -> Result<BTreeSet<Vec<u8>>, Box<dyn Error>> {
    let mut block = Parser::default()
        .parse(source)
        .map_err(|error| error.to_string())?;
    let mut literals = Literals::default();
    DefaultVisitor::visit_block(&mut block, &mut literals);
    Ok(literals.0)
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let artifact =
        PathBuf::from(arguments.next().ok_or(
            "usage: audit_strings <artifact> [--pool-only] [--source <original>] [--dump <new-file.tsv>]",
        )?);
    let mut original = None;
    let mut dump = None;
    let mut pool_only = false;
    while let Some(option) = arguments.next() {
        match option.to_str() {
            Some("--pool-only") => pool_only = true,
            Some("--source") => {
                original = Some(PathBuf::from(
                    arguments.next().ok_or("missing source path")?,
                ))
            }
            Some("--dump") => {
                dump = Some(PathBuf::from(arguments.next().ok_or("missing dump path")?))
            }
            _ => return Err(format!("unknown argument: {}", option.to_string_lossy()).into()),
        }
    }
    let started = Instant::now();
    let source = fs::read_to_string(&artifact)?;
    let result = if pool_only {
        let pool = extract(&source)?;
        adapted::ResultSet {
            values: pool.recovered.iter().map(|(_, s)| s.clone()).collect(),
            pool,
            ..Default::default()
        }
    } else {
        adapted::extract_adapted(&source)?
    };
    let audit = &result.pool;
    let elapsed = started.elapsed();
    let recovered = &result.values;
    println!("artifact={}", artifact.display());
    println!("artifact_bytes={}", source.len());
    println!("candidate_records={}", audit.candidates);
    println!("checksum_valid_records={}", audit.recovered.len());
    println!("checksum_rejected_records={}", audit.rejected);
    println!("unique_recovered_values={}", recovered.len());
    println!(
        "mode={}",
        if pool_only {
            "pool-only"
        } else {
            "adapted-static"
        }
    );
    println!("reconstructed_graphs={}", result.graphs);
    println!("rejected_graphs={}", result.graph_rejected);
    println!("decoded_vm_prototypes={}", result.prototypes);
    println!("decoded_vm_instructions={}", result.instructions);
    println!("rejected_vm_prototypes={}", result.vm_rejected);
    println!("decoded_bound_strings={}", result.bound_strings);
    println!("decoded_literal_recipes={}", result.recipe_strings);
    println!("interpreted_literal_recipe_steps={}", result.recipe_steps);
    println!(
        "interpreted_literal_memory_steps={}",
        result.recipe_memory_steps
    );
    println!("rejected_bound_strings={}", result.bound_rejected);
    println!("resolved_concatenations={}", result.concatenations);
    println!("read_parse_decode_ms={:.3}", elapsed.as_secs_f64() * 1000.0);
    if let Some(original) = original {
        // Only used after extraction, to check coverage; never used to decode.
        let literals = source_literals(&fs::read_to_string(original)?)?;
        println!("source_unique_string_literals={}", literals.len());
        println!(
            "source_literals_recovered={}",
            literals.intersection(recovered).count()
        );
        println!(
            "source_literals_missing={}",
            literals.difference(recovered).count()
        );
    }
    if let Some(path) = dump {
        let mut rows = String::from("id\tbyte_length\thex\tescaped_bytes\n");
        let rows_to_write: Vec<_> = if pool_only {
            audit
                .recovered
                .iter()
                .map(|(id, data)| (*id as usize, data))
                .collect()
        } else {
            recovered.iter().enumerate().collect()
        };
        for (id, data) in rows_to_write {
            write!(&mut rows, "{id}\t{}\t", data.len())?;
            for byte in data {
                write!(&mut rows, "{byte:02x}")?;
            }
            rows.push('\t');
            for byte in data {
                rows.extend(std::ascii::escape_default(*byte).map(char::from));
            }
            rows.push('\n');
        }
        // Do not overwrite evidence or an input file if the path is mistyped.
        use std::io::Write as _;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(rows.as_bytes())?;
        println!("dump={}", path.display());
    }
    if (audit.candidates == 0 && result.bound_strings == 0 && result.prototypes == 0)
        || audit.rejected != 0
        || result.graph_rejected != 0
        || result.vm_rejected != 0
        || result.bound_rejected != 0
    {
        return Err("no known string/VM data found, or recognized data failed validation".into());
    }
    Ok(())
}

fn main() {
    let result = stacker::maybe_grow(16 * 1024 * 1024, 32 * 1024 * 1024, run);
    if let Err(error) = result {
        eprintln!("audit failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use realobfuscator::{ObfuscationConfig, Obfuscator, ProtectionLevel};

    #[test]
    fn native_fragmentation_removes_whole_long_literals_from_the_pool_probe() {
        let literal =
            "a long literal whose individual pieces remain extractable from the shared pool";
        let source = format!("return '{literal}'");
        for seed in [0, 7, 42] {
            let base = ObfuscationConfig::default()
                .with_seed(seed)
                .with_level(ProtectionLevel::Maximum);
            for split in [false, true] {
                let mut config = base.clone();
                config.split_strings = split;
                let output = Obfuscator::new(config)
                    .obfuscate_with_report(&source)
                    .unwrap();
                let audit = extract(&output.code).unwrap();
                assert_eq!(audit.rejected, 0);
                assert!(!audit.recovered.is_empty());
                let whole = audit
                    .recovered
                    .iter()
                    .any(|(_, bytes)| bytes == literal.as_bytes());
                assert_eq!(whole, !split);
                assert_eq!(output.report.split_strings, usize::from(split));
                // Reassembling concatenation expressions is a straightforward
                // next step for an adapted probe; this is only a pool-only test.
            }
        }
    }

    #[test]
    fn recovers_packed_and_table_records_without_executing_lua() {
        let source = r#"return '', '\000\255\128', 'same', 'same', [==[long string]==]"#;
        let expected = source_literals(source).unwrap();
        for packed in [false, true] {
            let mut config = ObfuscationConfig::default()
                .with_seed(42)
                .with_level(ProtectionLevel::Paranoid);
            config.vm_pack_constants = packed;
            let output = Obfuscator::new(config)
                .obfuscate_with_report(source)
                .unwrap();
            let audit = extract(&output.code).unwrap();
            assert_eq!(audit.recovered.len(), output.report.encrypted_strings);
            assert_eq!(audit.rejected, 0);
            let recovered = audit
                .recovered
                .into_iter()
                .map(|(_, bytes)| bytes)
                .collect::<BTreeSet<_>>();
            assert!(expected.is_subset(&recovered));
        }
    }

    #[test]
    fn reports_corrupt_records_and_ignores_other_table_shapes() {
        let audit = extract("local a={[7]={'',1,2,3,1},[8]={'not a record'},[9]=42}").unwrap();
        assert_eq!(audit.candidates, 1);
        assert_eq!(audit.rejected, 1);
        assert!(audit.recovered.is_empty());
    }

    #[test]
    fn standalone_pool_extraction_stops_at_ciphertext_for_bound_literals() {
        for source in [
            "return 'first source-informed audit target','second source-informed audit target'",
            "local t={};function t:send(s)return s end;return t:send('first source-informed audit target'),t:send('second source-informed audit target')",
        ] {
        let expected = source_literals(source).unwrap();
        for seed in [0, 7, 42] {
            for packed in [false, true] {
                let mut config = ObfuscationConfig::hardened().with_seed(seed);
                config.vm_pack_constants = packed;
                let output = Obfuscator::new(config)
                    .obfuscate_with_report(source)
                    .unwrap();
                assert!(output.report.vm_bound_strings >= 2);
                let audit = extract(&output.code).unwrap();
                assert!(
                    !audit.recovered.is_empty(),
                    "the outer pool is still extractable"
                );
                assert_eq!(audit.rejected, 0);
                let recovered = audit
                    .recovered
                    .into_iter()
                    .map(|(_, bytes)| bytes)
                    .collect::<BTreeSet<_>>();
                assert!(expected.is_disjoint(&recovered));
                // This proves only that this pool-only probe needs another stage.
                // The prototype parameters and inverse algorithm remain in the artifact.
            }
        }
        }
    }
}
