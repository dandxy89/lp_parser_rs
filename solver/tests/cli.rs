//! Black-box tests of the `lp-highs` binary (protocol 1, section 2.9).

use std::io::Write;
use std::process::{Command, Output, Stdio};

use lp_highs::protocol::{DiagnosisMethod, ErrorKind, Event, HelperInfo, PROTOCOL_VERSION, Phase, SolveRequest, SolveResponse};

fn helper() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lp-highs"))
}

fn run(args: &[&str], stdin: &str) -> Output {
    let mut child =
        helper().args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("the helper starts");
    child.stdin.take().expect("stdin is piped").write_all(stdin.as_bytes()).expect("stdin accepts the request");
    child.wait_with_output().expect("the helper exits")
}

fn solve(request: &SolveRequest) -> (Output, SolveResponse) {
    let output = run(&["solve"], &serde_json::to_string(request).expect("serialises"));
    let text = String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8");
    assert_eq!(text.lines().count(), 1, "exactly one response line, got {text:?}");
    let response = SolveResponse::from_json(&text).unwrap_or_else(|error| panic!("invalid response {text:?}: {error}"));
    (output, response)
}

fn lp(text: &str) -> SolveRequest {
    SolveRequest::new(lp_highs::protocol::Format::Lp, text)
}

#[test]
fn info_describes_the_helper() {
    let output = run(&["info"], "");
    assert_eq!(output.status.code(), Some(0));
    let info: HelperInfo = serde_json::from_slice(&output.stdout).expect("info is one JSON object");
    assert_eq!(info.name, "lp-highs");
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(info.protocol, PROTOCOL_VERSION);
    assert!(info.highs.starts_with(|c: char| c.is_ascii_digit()), "HiGHS version looks like 1.x.y: {}", info.highs);
}

#[test]
fn version_flag_prints_the_version() {
    let output = run(&["--version"], "");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), format!("lp-highs {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn unknown_usage_exits_two() {
    assert_eq!(run(&[], "").status.code(), Some(2));
    assert_eq!(run(&["frobnicate"], "").status.code(), Some(2));
}

#[test]
fn an_optimal_lp_reports_objective_and_duals() {
    let (output, response) = solve(&lp("Maximize\n obj: 3 x + 2 y\nSubject To\n cap: x + y <= 4\n lim: x + 3 y <= 6\nEnd"));
    assert_eq!(output.status.code(), Some(0));
    let result = response.result.expect("success");
    assert_eq!(result.status, "Optimal");
    assert!((result.objective_value.expect("objective") - 12.0).abs() < 1e-6);
    let cap = result.constraints.iter().find(|c| c.name == "cap").expect("cap row");
    assert!(cap.dual.is_some_and(|dual| dual > 0.0), "an LP carries duals: {cap:?}");
    assert!(result.variables.iter().all(|v| v.reduced_cost.is_some()));
}

#[test]
fn a_mip_carries_no_duals() {
    let (_, response) = solve(&lp("Maximize\n obj: x + y\nSubject To\n c1: 2 x + 3 y <= 12\nGenerals\n x y\nEnd"));
    let result = response.result.expect("success");
    assert_eq!(result.status, "Optimal");
    assert!(result.constraints.iter().all(|c| c.dual.is_none()));
    assert!(result.variables.iter().all(|v| v.reduced_cost.is_none()));
}

#[test]
fn an_infeasible_lp_yields_an_iis_naming_both_rows_and_exits_zero() {
    let (output, response) = solve(&lp("Minimize\n obj: x\nSubject To\n c1: x >= 2\n c2: x <= 1\nEnd"));
    assert_eq!(output.status.code(), Some(0), "a written response means exit 0, infeasible included");
    let result = response.result.expect("success");
    assert_eq!(result.status, "Infeasible");
    let diagnosis = result.infeasibility.expect("diagnose defaults to auto");
    assert_eq!(diagnosis.method, DiagnosisMethod::Iis);
    let mut names: Vec<&str> = diagnosis.constraints.iter().map(|c| c.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["c1", "c2"]);
}

#[test]
fn an_unsupported_model_is_an_unsuccessful_response_not_a_crash() {
    let source = "Minimize\n obj: x\nSubject To\n c1: x >= 1\n ind: b = 1 -> x >= 2\nBinary\n b\nEnd";
    let (output, response) = solve(&lp(source));
    assert_eq!(output.status.code(), Some(0));
    assert!(!response.ok);
    assert_eq!(response.error.expect("error").kind, ErrorKind::UnsupportedModel);
}

#[test]
fn an_unparsable_model_is_a_parse_error() {
    let (_, response) = solve(&lp("this is not an LP file"));
    assert_eq!(response.error.expect("error").kind, ErrorKind::ParseError);
}

#[test]
fn malformed_stdin_exits_two_with_a_protocol_error() {
    let output = run(&["solve"], "this is not json");
    assert_eq!(output.status.code(), Some(2));
    let response = SolveResponse::from_json(&String::from_utf8_lossy(&output.stdout)).expect("a protocol error response is still written");
    assert_eq!(response.error.expect("error").kind, ErrorKind::Protocol);
    assert!(!output.stderr.is_empty(), "the reason is also on stderr");
}

#[test]
fn a_foreign_protocol_version_exits_two() {
    let output = run(&["solve"], r#"{"protocol":2,"format":"lp","text":"min\nobj: x\nend"}"#);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported protocol 2"));
}

#[test]
fn phase_events_arrive_on_stderr_in_order() {
    let (output, _) = solve(&lp("Minimize\n obj: x\nSubject To\n c1: x >= 2\n c2: x <= 1\nEnd"));
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    let events: Vec<Event> = stderr.lines().filter_map(Event::from_line).collect();
    let phases: Vec<Phase> = events.iter().map(|Event::Phase { phase, .. }| *phase).collect();
    assert_eq!(phases, [Phase::Parsing, Phase::Building, Phase::Solving, Phase::Extracting, Phase::Diagnosing], "stderr was {stderr:?}");
    let times: Vec<u64> = events.iter().map(|Event::Phase { elapsed_ms, .. }| *elapsed_ms).collect();
    assert!(times.windows(2).all(|pair| pair[0] <= pair[1]), "elapsed time never goes backwards: {times:?}");
}

#[test]
fn highs_opt_next_to_the_model_applies_to_the_solve() {
    let dir = std::env::temp_dir().join(format!("lp_highs_cli_opt_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("highs.opt"), "simplex_strategy = 1\n").expect("options file");

    let mut request = lp("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\nEnd");
    request.include_log = true;
    let mut child = helper()
        .arg("solve")
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the helper starts");
    child.stdin.take().expect("stdin").write_all(serde_json::to_string(&request).expect("serialises").as_bytes()).expect("request written");
    let output = child.wait_with_output().expect("exits");
    std::fs::remove_dir_all(&dir).expect("cleanup");

    let response = SolveResponse::from_json(&String::from_utf8_lossy(&output.stdout)).expect("response");
    let log = response.result.expect("success").log.expect("log requested");
    assert!(log.contains("simplex_strategy = 1"), "applied options appear at the top of the log: {log}");
}
