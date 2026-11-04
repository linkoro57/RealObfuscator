use std::{fs, process::Command, time::{SystemTime, UNIX_EPOCH}};

fn temp_dir() -> std::path::PathBuf {
    let n=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let p=std::env::temp_dir().join(format!("realobfuscator-{n}"));fs::create_dir_all(&p).unwrap();p
}

#[test]
fn cli_obfuscates_a_file_and_is_reproducible(){
    let dir=temp_dir();let input=dir.join("input.luau");let a=dir.join("a.luau");let b=dir.join("b.luau");
    fs::write(&input,"local message='hello';return message,17").unwrap();
    for output in [&a,&b]{let status=Command::new(env!("CARGO_BIN_EXE_realobfuscator")).args([input.to_str().unwrap(),"-o",output.to_str().unwrap(),"--mode","native","--seed","42","--quiet"]).status().unwrap();assert!(status.success());}
    assert_eq!(fs::read(&a).unwrap(),fs::read(&b).unwrap());assert!(fs::read_to_string(&a).unwrap().starts_with("--!nocheck\n--!nolint"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_rejects_an_input_output_collision(){
    let dir=temp_dir();let input=dir.join("input.lua");fs::write(&input,"return 1").unwrap();
    let status=Command::new(env!("CARGO_BIN_EXE_realobfuscator")).args([input.to_str().unwrap(),"-o",input.to_str().unwrap()]).status().unwrap();
    assert_eq!(status.code(),Some(2));assert_eq!(fs::read_to_string(&input).unwrap(),"return 1");fs::remove_dir_all(dir).unwrap();
}
