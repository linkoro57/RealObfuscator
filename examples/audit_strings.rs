//! Static audit of RealObfuscator's public string-record format. Does not execute Lua.
//! This is a source-informed regression probe, not a general-purpose deobfuscator.
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
            "usage: audit_strings <artifact> [--source <original>] [--dump <new-file.tsv>]",
        )?);
    let mut original = None;
    let mut dump = None;
    while let Some(option) = arguments.next() {
        match option.to_str() {
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
    let audit = extract(&source)?;
    let elapsed = started.elapsed();
    let recovered: BTreeSet<_> = audit
        .recovered
        .iter()
        .map(|(_, data)| data.clone())
        .collect();
    println!("artifact={}", artifact.display());
    println!("artifact_bytes={}", source.len());
    println!("candidate_records={}", audit.candidates);
    println!("checksum_valid_records={}", audit.recovered.len());
    println!("checksum_rejected_records={}", audit.rejected);
    println!("unique_recovered_values={}", recovered.len());
    println!("read_parse_decode_ms={:.3}", elapsed.as_secs_f64() * 1000.0);
    if let Some(original) = original {
        // Only used after extraction, to check coverage; never used to decode.
        let literals = source_literals(&fs::read_to_string(original)?)?;
        println!("source_unique_string_literals={}", literals.len());
        println!(
            "source_literals_recovered={}",
            literals.intersection(&recovered).count()
        );
        println!(
            "source_literals_missing={}",
            literals.difference(&recovered).count()
        );
    }
    if let Some(path) = dump {
        let mut rows = String::from("id\tbyte_length\thex\tescaped_bytes\n");
        for (id, data) in &audit.recovered {
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
    if audit.candidates == 0 || audit.rejected != 0 {
        return Err("no known string records found, or some candidate checksums failed".into());
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
}
