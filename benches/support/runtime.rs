//! Fresh-process load measurements and in-process warm execution are distinct.
use super::measurement::Measurement;
use crate::Result;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct Runtime {
    executable: PathBuf,
    directory: PathBuf,
    processes: usize,
    samples: usize,
    timeout: Duration,
}

pub struct RuntimeResult {
    pub checksum: String,
    pub process_wall: Measurement,
    pub module_load: Measurement,
    pub warm_execution: Measurement,
}

impl Runtime {
    pub fn new(
        directory: &Path,
        executable: &Path,
        processes: usize,
        samples: usize,
        iterations: usize,
        timeout: Duration,
    ) -> Result<Self> {
        // Canonicalize before setting the child's working directory.
        let executable = fs::canonicalize(executable)?;
        fs::create_dir(directory)?;
        let directory = fs::canonicalize(directory)?;
        let driver = format!(
            r#"
local clock=os.clock
local before=clock()
local workload=require('./module')
local load=clock()-before
local expected=workload({iterations})
assert(type(expected)=='number' and expected==expected,'invalid checksum')
assert(workload({iterations})==expected,'non-repeatable workload')
print('ROBENCH_RESULT',string.format('%.17g',expected),load)
for sample=1,{samples} do
    local start=clock()
    local result=workload({iterations})
    local elapsed=clock()-start
    assert(result==expected,'checksum changed during measurement')
    print('ROBENCH_TIME',elapsed)
end
"#
        );
        fs::write(directory.join("driver.luau"), driver)?;
        Ok(Self {
            executable,
            directory,
            processes,
            samples,
            timeout,
        })
    }

    pub fn measure(&self, source: &str, expected: Option<&str>) -> Result<RuntimeResult> {
        fs::write(self.directory.join("module.luau"), source)?;
        let mut wall = Vec::new();
        let mut load = Vec::new();
        let mut warm = Vec::new();
        let mut checksum = expected.map(str::to_owned);
        for _ in 0..self.processes {
            let (elapsed, stdout) = self.run()?;
            let mut result = None;
            let mut execution_samples = 0;
            for line in stdout.lines() {
                let fields: Vec<_> = line.split_whitespace().collect();
                match fields.as_slice() {
                    ["ROBENCH_RESULT", value, seconds] if result.is_none() => {
                        let numeric: f64 = value.parse()?;
                        if !numeric.is_finite() {
                            return Err("non-finite runtime checksum".into());
                        }
                        result = Some((*value).to_owned());
                        load.push(milliseconds(seconds)?);
                    }
                    ["ROBENCH_TIME", seconds] => {
                        warm.push(milliseconds(seconds)?);
                        execution_samples += 1;
                    }
                    _ => return Err(format!("unexpected Luau benchmark output: {line}").into()),
                }
            }
            let result = result.ok_or("runtime omitted its checksum")?;
            if execution_samples != self.samples {
                return Err(format!(
                    "expected {} runtime samples, got {execution_samples}",
                    self.samples
                )
                .into());
            }
            if let Some(expected) = &checksum {
                if result != *expected {
                    return Err(
                        format!("checksum mismatch: expected {expected}, got {result}").into(),
                    );
                }
            } else {
                checksum = Some(result);
            }
            wall.push(elapsed.as_secs_f64() * 1000.0);
        }
        Ok(RuntimeResult {
            checksum: checksum.ok_or("no runtime processes requested")?,
            process_wall: Measurement {
                samples_ms: wall,
                batch: 1,
            },
            module_load: Measurement {
                samples_ms: load,
                batch: 1,
            },
            warm_execution: Measurement {
                samples_ms: warm,
                batch: 1,
            },
        })
    }

    fn run(&self) -> Result<(Duration, String)> {
        let start = Instant::now();
        let mut child = Command::new(&self.executable)
            .current_dir(&self.directory)
            .arg("driver.luau")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        // Drain both streams concurrently; a diagnostic must never block the child.
        let stdout = child.stdout.take().ok_or("missing stdout pipe")?;
        let stderr = child.stderr.take().ok_or("missing stderr pipe")?;
        let stdout = thread::spawn(move || read_all(stdout));
        let stderr = thread::spawn(move || read_all(stderr));
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if start.elapsed() >= self.timeout {
                timed_out = true;
                let _ = child.kill();
                break child.wait()?;
            }
            thread::sleep(Duration::from_millis(1));
        };
        let elapsed = start.elapsed();
        let stdout = stdout.join().map_err(|_| "stdout reader panicked")??;
        let stderr = stderr.join().map_err(|_| "stderr reader panicked")??;
        if timed_out || !status.success() {
            return Err(format!(
                "Luau {} ({status}): {}",
                if timed_out { "timed out" } else { "failed" },
                String::from_utf8_lossy(&stderr)
            )
            .into());
        }
        Ok((elapsed, String::from_utf8(stdout)?))
    }
}

fn read_all(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn milliseconds(value: &str) -> Result<f64> {
    let seconds: f64 = value.parse()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return Err("invalid runtime duration".into());
    }
    Ok(seconds * 1000.0)
}

pub fn locate() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("LUAU_BIN") {
        return Ok(fs::canonicalize(path)?);
    }
    let name = if cfg!(windows) { "luau.exe" } else { "luau" };
    let local = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/runtime/0.741")
        .join(name);
    if local.is_file() {
        return Ok(local);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Ok(fs::canonicalize(candidate)?);
            }
        }
    }
    Err("Luau is required by --runtime; set LUAU_BIN to its executable path".into())
}
