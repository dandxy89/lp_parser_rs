use std::path::PathBuf;

use congenial_enigma::{
    model::{LPDefinition, Sense},
    parse::{parse_file, parse_lp_file},
};

#[test]
fn afiro() {
    let result = read_file_from_resources("afiro.lp").unwrap();
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 3);
    assert_eq!(result.constraints.len(), 27);
}

#[test]
fn boeing1() {
    let result = read_file_from_resources("boeing1.lp").unwrap();
    assert_eq!(result.problem_sense, Sense::Minimize);
}

#[test]
fn boeing2() {
    let result = read_file_from_resources("boeing2.lp").unwrap();
    assert_eq!(result.problem_sense, Sense::Minimize);
}

#[test]
fn fit1d() {
    let result = read_file_from_resources("fit1d.lp").unwrap();
    assert_eq!(result.problem_sense, Sense::Minimize);
}

#[test]
fn fit2d() {
    let result = read_file_from_resources("fit2d.lp").unwrap();
    assert_eq!(result.problem_sense, Sense::Minimize);
}

#[test]
fn kb2() {
    let result = read_file_from_resources("kb2.lp").unwrap();
    assert_eq!(result.problem_sense, Sense::Minimize);
}

#[test]
fn pulp() {
    let result = read_file_from_resources("pulp.lp").unwrap();
    assert_eq!(result.problem_sense, Sense::Minimize);
}

#[test]
fn pulp2() {
    let result = read_file_from_resources("pulp2.lp").unwrap();
    assert_eq!(result.problem_sense, Sense::Minimize);
}

#[test]
fn pulp3() {
    let result = read_file_from_resources("pulp3.lp").unwrap();
    assert_eq!(result.problem_sense, Sense::Maximize);
}

#[test]
fn sc50a() {
    let result = read_file_from_resources("sc50a.lp").unwrap();
    assert_eq!(result.problem_sense, Sense::Minimize);
}

fn read_file_from_resources(file_name: &str) -> anyhow::Result<LPDefinition> {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push(format!("resources/{file_name}"));
    let contents = parse_file(&file_path)?;
    parse_lp_file(&contents)
}
