extern crate clap;
extern crate yaml_rust;
extern crate sha1;

use std::env;
use std::path::PathBuf;
use std::fs::File;
use std::error::Error;
use std::io::prelude::*;
use std::io::BufReader;

use clap::{App, Arg, SubCommand, ArgMatches};
use yaml_rust::{YamlLoader};

const SIGIL: &str = "# ENVHASH:";

fn main() -> Result<(), Box<Error>> {
    let app_m = App::new("conda-lockfile")
        .arg(Arg::with_name("v")
             .short("v")
             .multiple(true)
             .help("Sets the level of verbosity"))
        .subcommand(SubCommand::with_name("freeze")
                    .arg(Arg::with_name("depfile")
                         .default_value("deps.yml")
                         )
                    .arg(Arg::with_name("lockfile")
                         .default_value("deps.yml")
                         )
                    .arg(Arg::with_name("platform")
                         )
                    )
        .subcommand(SubCommand::with_name("create")
                    .arg(Arg::with_name("lockfile")
                         .default_value("deps.yml")
                         )
                    .arg(Arg::with_name("platform")
                         )
                    )
        .subcommand(SubCommand::with_name("checkenv")
                    .arg(Arg::with_name("depfile")
                         .default_value("deps.yml")))
        .subcommand(SubCommand::with_name("checklocks")
                    .arg(Arg::with_name("depfile")
                         .default_value("deps.yml")))
        .get_matches();

    match app_m.occurrences_of("v") {
        0 => println!("Only output on errors"),
        1 => println!("Info-level verbosity"),
        2 => println!("Debug-level verbosity"),
        3 | _ => println!("Don't be crazy"),
    }

    let val = match app_m.subcommand() {
        ("freeze",   Some(sub_m)) => handle_freeze(sub_m),
        ("create",   Some(sub_m)) => handle_create(sub_m),
        ("checkenv",  Some(sub_m)) => handle_checkenv(sub_m),
        ("checklocks",   Some(sub_m)) => handle_checklocks(sub_m),
        _ => Ok(()),
    };
    val
}


fn handle_freeze(matches: &ArgMatches) -> Result<(), Box<Error>> {
    println!("{:?}", matches);
    Ok(())
}


fn handle_create(matches: &ArgMatches) -> Result<(), Box<Error>> {
    println!("{:?}", matches);
    Ok(())
}


fn get_hash(line: &str) -> Option<&str> {
    if line.starts_with(SIGIL) {
        Some(&line[10..])
    } else {
        None
    }
}


fn handle_checkenv(matches: &ArgMatches) -> Result<(), Box<Error>> {
    // Get the data from the depfile.
    let depfile_path = matches.value_of("depfile").expect("bah!");
    let mut depfile = File::open(depfile_path).expect("fille not found");
    let mut depfile_data = String::new();
    depfile.read_to_string(&mut depfile_data).expect("Could not read file");

    // Hash the contents of the file
    let mut m = sha1::Sha1::new();
    m.update(depfile_data.as_bytes());
    let expected_hash = m.digest().to_string();

    // Extract the name of the environment
    let docs = YamlLoader::load_from_str(&depfile_data).unwrap();
    let doc = &docs[0];  // YamlLoader loads multiple documents
    let env_name = doc["name"].as_str().unwrap();
    println!("env name: {}", env_name);

    let root = env::var("CONDA_ROOT").unwrap();
    let lockfile_path: PathBuf = [
        &root,
        "envs",
        env_name,
        "deps.yml.lock",
    ].iter().collect();
    println!("lockfile_path: {}", lockfile_path.to_str().unwrap());

    let mut lockfile = File::open(lockfile_path).expect("file not found");
    let mut lockfile_data = String::new();
    lockfile.read_to_string(&mut lockfile_data).expect("Unable to read lockfile");
    let mut hashes = lockfile_data.lines()
        .filter_map(|line: &str| get_hash(&line))
        .map(|line| line.trim());
    let found_hash = hashes.next().unwrap();

    println!("{}", expected_hash);
    println!("{}", found_hash);
    println!("{}", expected_hash == found_hash);

    Ok(())
}


fn handle_checklocks(matches: &ArgMatches) -> Result<(), Box<Error>> {
    println!("{:?}", matches);
    Ok(())
}
