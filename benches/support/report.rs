use super::measurement::{Measurement, Statistics};
use crate::Result;
use realobfuscator::ObfuscationReport;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};

pub struct Case<'a> {
    pub workload: &'a str,
    pub scale: usize,
    pub profile: &'a str,
    pub seed: u64,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub report: &'a ObfuscationReport,
}

pub struct Report {
    summary: BufWriter<File>,
    raw: BufWriter<File>,
}

impl Report {
    /// Refuse to reuse an existing run directory, including partial previous runs.
    pub fn create(directory: &Path, metadata: &str) -> Result<Self> {
        if let Some(parent) = directory.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        fs::create_dir(directory)?;
        let open = |name| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
        };
        let mut metadata_file = open("metadata.txt")?;
        metadata_file.write_all(metadata.as_bytes())?;
        let mut summary = BufWriter::new(open("summary.csv")?);
        writeln!(summary, "kind,workload,scale,profile,seed,samples,batch,input_bytes,output_bytes,expansion,min_ms,median_ms,p95_ms,max_ms,mean_ms,stddev_ms,input_mib_per_sec,renamed_bindings,encrypted_strings,string_references,obfuscated_numbers,hidden_fields,flattened_regions,vm_chunks,vm_functions,vm_instructions,vm_opcode_variants,vm_fused_instructions,vm_native_functions,vm_native_bridges,vm_iteration_bridges,vm_closure_bridges,integrity_checks,decoy_blocks,vm_chunk_fallback,vm_fallback_reasons")?;
        let mut raw = BufWriter::new(open("samples.csv")?);
        writeln!(
            raw,
            "kind,workload,scale,profile,seed,sample,batch,elapsed_ms"
        )?;
        Ok(Self { summary, raw })
    }

    pub fn record(&mut self, kind: &str, case: &Case<'_>, measurement: &Measurement) -> Result<()> {
        let stats = Statistics::from_samples(&measurement.samples_ms);
        let report = case.report;
        let expansion = if case.input_bytes == 0 {
            0.0
        } else {
            case.output_bytes as f64 / case.input_bytes as f64
        };
        let throughput = if stats.median == 0.0 {
            0.0
        } else {
            case.input_bytes as f64 / 1_048_576.0 / (stats.median / 1000.0)
        };
        let reasons = report
            .vm_fallback_reasons
            .iter()
            .map(|(reason, count)| format!("{reason}: {count}"))
            .collect::<Vec<_>>()
            .join("; ");
        let mut fields = vec![
            kind.into(),
            case.workload.into(),
            case.scale.to_string(),
            case.profile.into(),
            case.seed.to_string(),
            measurement.samples_ms.len().to_string(),
            measurement.batch.to_string(),
            case.input_bytes.to_string(),
            case.output_bytes.to_string(),
            format!("{expansion:.6}"),
        ];
        fields.extend(
            [
                stats.min,
                stats.median,
                stats.p95,
                stats.max,
                stats.mean,
                stats.stddev,
            ]
            .map(|n| format!("{n:.9}")),
        );
        fields.push(if matches!(kind, "obfuscate" | "lex" | "emit") {
            format!("{throughput:.9}")
        } else {
            String::new()
        });
        fields.extend(
            [
                report.renamed_bindings,
                report.encrypted_strings,
                report.string_references,
                report.obfuscated_numbers,
                report.hidden_fields,
                report.flattened_regions,
                report.vm_chunks,
                report.vm_functions,
                report.vm_instructions,
                report.vm_opcode_variants,
                report.vm_fused_instructions,
                report.vm_native_functions,
                report.vm_native_bridges,
                report.vm_iteration_bridges,
                report.vm_closure_bridges,
                report.integrity_checks,
                report.decoy_blocks,
            ]
            .map(|n| n.to_string()),
        );
        fields.push(report.vm_chunk_fallback.clone().unwrap_or_default());
        fields.push(reasons);
        write_row(&mut self.summary, &fields)?;
        for (index, sample) in measurement.samples_ms.iter().enumerate() {
            write_row(
                &mut self.raw,
                &[
                    kind.into(),
                    case.workload.into(),
                    case.scale.to_string(),
                    case.profile.into(),
                    case.seed.to_string(),
                    index.to_string(),
                    measurement.batch.to_string(),
                    format!("{sample:.9}"),
                ],
            )?;
        }
        // Keep completed measurements usable after an interrupted long matrix.
        self.summary.flush()?;
        self.raw.flush()?;
        Ok(())
    }
}

fn write_row(writer: &mut impl Write, fields: &[String]) -> std::io::Result<()> {
    let row = fields
        .iter()
        .map(|field| format!("\"{}\"", field.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",");
    writeln!(writer, "{row}")
}
