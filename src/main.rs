extern crate clap;
extern crate glob;
extern crate sha1;
extern crate yaml_rust;

use std::str;
use std::env;
use std::error::Error;
use std::fs::{copy, File};
use std::io::prelude::*;
use std::io::{Error as ioError, ErrorKind as ioErrorKind};
use std::path::PathBuf;
use std::process::Command;

use clap::{App, Arg, ArgMatches, SubCommand};
use glob::glob;
use yaml_rust::{Yaml, YamlLoader, YamlEmitter};

const SIGIL: &str = "# ENVHASH:";

type Result<T> = std::result::Result<T, Box<Error>>;

fn main() -> Result<()> {
    let app_m = App::new("conda-lockfile")
        .arg(
            Arg::with_name("v")
                .short("v")
                .multiple(true)
                .help("Sets the level of verbosity"),
        ).subcommand(
            SubCommand::with_name("freeze")
                .arg(Arg::with_name("depfile").default_value("deps.yml"))
                .arg(Arg::with_name("lockfile").default_value("deps.yml"))
                .arg(Arg::with_name("platform")),
        ).subcommand(
            SubCommand::with_name("create")
                .arg(Arg::with_name("lockfile").default_value("deps.yml"))
                .arg(Arg::with_name("platform")),
        ).subcommand(
            SubCommand::with_name("checkenv")
                .arg(Arg::with_name("depfile").default_value("deps.yml")),
        ).subcommand(
            SubCommand::with_name("checklocks")
                .arg(Arg::with_name("depfile").default_value("deps.yml"))
                .arg(Arg::with_name("depfiles").multiple(true)),
        ).get_matches();

    match app_m.occurrences_of("v") {
        0 => println!("Only output on errors"),
        1 => println!("Info-level verbosity"),
        2 => println!("Debug-level verbosity"),
        3 | _ => println!("Don't be crazy"),
    }

    let val = match app_m.subcommand() {
        ("freeze", Some(sub_m)) => handle_freeze(sub_m),
        ("create", Some(sub_m)) => handle_create(sub_m),
        ("checkenv", Some(sub_m)) => handle_checkenv(sub_m),
        ("checklocks", Some(sub_m)) => handle_checklocks(sub_m),
        _ => Ok(()),
    };
    val
}

fn handle_freeze(matches: &ArgMatches) -> Result<()> {
    let depfile_path = matches.value_of("depfile").unwrap();

    let execution_platform = get_platform()?;
    let target_platform = match matches.value_of("platform") {
        Some(platform) => platform,
        None => &execution_platform,
    };

    // TODO: this might not be the correct path when cross-building.
    if execution_platform == target_platform {
        let lockfile_path = extract_lockfile_path(&matches);
        return freeze_same_platform(&depfile_path, &lockfile_path);
    }

    match (execution_platform.as_str(), target_platform) {
        ("Darwin", "Linux") => {
            let lockfile_path = "TODO".to_string();
            freeze_linux_on_mac(&depfile_path, &lockfile_path)
        },
        _ => {
            let msg = format!(
                "Unable to target {} from {}",
                target_platform, execution_platform
            );
            Err(ioError::new(ioErrorKind::Other, msg).into())
        }
    }
}

fn freeze_same_platform(depfile_path: &str, lockfile_path: &str) -> Result<()> {
    let depfile = File::open(depfile_path)?;
    let env_hash = compute_file_hash(depfile)?;

    // Extract the name of the environment
    let depfile2 = File::open(depfile_path)?;
    let env_spec = read_conda_yaml_data(depfile2)?;
    let env_name = env_spec["name"].as_str().unwrap();

    let conda_path = find_conda()?;
    // Create the environment, but use a name that is unlikely to clobber anything pre-existing.
    let tmp_name = "___conda_lockfile_temp".to_string();
    Command::new(&conda_path)
        .args(&["env", "crate", "-f", &depfile_path, "-n", &tmp_name, "--force"])
        .output()?;

    // Read the env create by `conda create`.
    let output = Command::new(&conda_path)
        .args(&["env", "export", "-n", &tmp_name])
        .output()?;
    let lock_data = str::from_utf8(&output.stdout)?;
    println!("{}", lock_data);

    // Replace the temporary env name with the real one.
    // Also drop the prefix field.  It is irrelevant.
    let mut docs = YamlLoader::load_from_str(lock_data)?;
    let doc = docs.remove(0);
    let mut data_hash = doc.into_hash().unwrap();
    data_hash.insert(Yaml::from_str("name"), Yaml::from_str(&env_name));
    data_hash.remove(&Yaml::from_str("prefix"));
    let lock_spec = Yaml::Hash(data_hash);

    let lockfile = File::create(lockfile_path)?;
    write_lockfile(lockfile, &lock_spec, &env_hash)?;
    Ok(())
}

fn write_lockfile<W: Write>(mut lockfile: W, lock_spec: &Yaml, env_hash: &str) -> Result<()> {
    let mut serialized_data = String::new();
    {
        let mut emitter = YamlEmitter::new(&mut serialized_data);
        emitter.dump(&lock_spec)?;
    }

    let env_hash_line = format!("{} {}\n", SIGIL, env_hash);
    lockfile.write_all(env_hash_line.as_bytes())?;
    lockfile.write_all(serialized_data.as_bytes())?;
    Ok(())
}

fn freeze_linux_on_mac(_depfile_path: &str, _lockfile_path: &str) -> Result<()> {
    Ok(())
}

fn extract_lockfile_path(matches: &ArgMatches) -> String {
    match matches.value_of("lockfile") {
        Some(path) => path.to_string(),
        None => default_lockfile(),
    }
}

fn default_lockfile() -> String {
    match get_platform() {
        Ok(platform) => format!("deps.yml.{}.lock", platform),
        Err(_) => "".to_string(),
    }
}

fn conda_prefix(name: &str) -> Result<PathBuf> {
    let root = env::var("CONDA_ROOT")?;
    let path: PathBuf = [&root, "envs", name].iter().collect();
    Ok(path)
}

fn get_platform() -> Result<String> {
    if cfg!(target_os = "linux") {
        Ok("Linux".to_string())
    } else if cfg!(target_os = "macos") {
        Ok("Darwin".to_string())
    } else {
        Err(ioError::new(ioErrorKind::Other, "Unknown platform").into())
    }
}

fn find_conda() -> Result<String> {
    match env::var("CONDA_EXE") {
        Ok(conda) => Ok(conda),
        Err(_) => match env::var("_CONDA_EXE") {
            Ok(conda) => Ok(conda),
            Err(_) => Err(ioError::new(ioErrorKind::Other, "Unable to find conda").into()),
        },
    }
}

fn handle_create(matches: &ArgMatches) -> Result<()> {
    if cfg!(target_os = "windows") {
        return Err(ioError::new(ioErrorKind::Other, "Unsupported os").into());
    }

    let lockfile_path = extract_lockfile_path(&matches);
    let lockfile = File::open(&lockfile_path)?;
    let doc = read_conda_yaml_data(lockfile)?;
    let env_name = doc["name"].as_str().unwrap();

    let conda_path = find_conda()?;
    println!("conda_path {}", conda_path);
    let output = Command::new(conda_path)
        .args(&[
            "env",
            "create",
            "--force",
            "-q",
            "--json",
            "--name",
            &env_name,
            "-f",
            &lockfile_path,
        ]).output()?;
    println!("{:?}", output);

    // Copy lockfile to constructed env
    let mut embeded_lockfile = conda_prefix(&env_name)?;
    embeded_lockfile.push("deps.yml.lock");
    copy(lockfile_path, embeded_lockfile)?;
    Ok(())
}

fn read_sigil_hash<R: Read>(mut f: R) -> Result<String> {
    let mut file_data = String::new();
    f.read_to_string(&mut file_data)?;
    let hash = file_data
        .lines()
        .filter_map(|line| {
            if line.starts_with(SIGIL) {
                Some(&line[10..])
            } else {
                None
            }
        }).map(|line| line.trim())
        .nth(0);
    match hash {
        Some(hash) => Ok(hash.to_string()),
        None => Err(ioError::new(ioErrorKind::Other, "No Hashes in file").into()),
    }
}

fn compute_file_hash<R: Read>(mut f: R) -> Result<String> {
    let mut depfile_data = String::new();
    f.read_to_string(&mut depfile_data)?;

    // Hash the contents of the file
    let mut m = sha1::Sha1::new();
    m.update(depfile_data.as_bytes());
    Ok(m.digest().to_string())
}

fn read_conda_yaml_data<R: Read>(mut f: R) -> Result<Yaml> {
    let mut depfile_data = String::new();
    f.read_to_string(&mut depfile_data)?;
    let mut docs = YamlLoader::load_from_str(&depfile_data).unwrap();
    let doc = docs.remove(0); // YamlLoader loads multiple documents.  We only want the first.
    Ok(doc)
}

fn handle_checkenv(matches: &ArgMatches) -> Result<()> {
    // Get the data from the depfile.
    let depfile_path = matches.value_of("depfile").unwrap();
    let depfile = File::open(depfile_path)?;
    let expected_hash = compute_file_hash(depfile)?;

    // Extract the name of the environment
    let depfile2 = File::open(depfile_path)?;
    let doc = read_conda_yaml_data(depfile2)?;
    let env_name = doc["name"].as_str().unwrap();
    println!("env name: {}", env_name);

    let root = env::var("CONDA_ROOT").unwrap();
    let lockfile_path: PathBuf = [&root, "envs", env_name, "deps.yml.lock"].iter().collect();
    println!("lockfile_path: {}", lockfile_path.to_str().unwrap());

    let lockfile = File::open(lockfile_path)?;
    let found_hash = read_sigil_hash(lockfile)?;

    if found_hash == expected_hash {
        Ok(())
    } else {
        println!("{}", expected_hash);
        println!("{}", found_hash);
        println!("{}", expected_hash == found_hash);
        Err(ioError::new(ioErrorKind::Other, "Hashes do not match").into())
    }
}

fn find_lockfiles() -> Vec<PathBuf> {
    let glob_paths: Vec<PathBuf> = glob("deps.yml.*.lock")
        .expect("Failed to read glob pattern")
        .map(|x| x.unwrap())
        .collect();
    glob_paths
}

fn handle_checklocks(matches: &ArgMatches) -> Result<()> {
    let depfile_path = matches.value_of("depfile").unwrap();
    let depfile = File::open(depfile_path)?;
    let expected_hash = compute_file_hash(depfile)?;

    let lockfiles = match matches.values_of("lockfiles") {
        Some(files) => files.map(|p| PathBuf::from(p)).collect(),
        None => find_lockfiles(),
    };

    let mut success = true;
    for lockfile_path in lockfiles {
        let lockfile = File::open(&lockfile_path)?;
        let found_hash = read_sigil_hash(lockfile)?;
        if found_hash != expected_hash {
            success = false;
            println!(
                "Hashes do not match {:?}, {:?}",
                depfile_path, lockfile_path
            );
            println!("lock    hash: {}", found_hash);
            println!("depfile hash: {}", expected_hash);
        }
    }

    if success {
        Ok(())
    } else {
        Err(ioError::new(ioErrorKind::Other, "Hashes do not match").into())
    }
}
