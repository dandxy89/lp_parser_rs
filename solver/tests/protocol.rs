//! Wire-format tests: every fixture round-trips through the serde types
//! unchanged, and a foreign protocol version is rejected.

use std::path::PathBuf;

use lp_highs::protocol::{Diagnose, ErrorKind, Event, Format, Phase, Presolve, SolveRequest, SolveResponse};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/protocol").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Deserialise `name` into `T`, serialise it back and require the same JSON value.
fn round_trip<T: DeserializeOwned + Serialize>(name: &str) -> T {
    let text = fixture(name);
    let original: Value = serde_json::from_str(&text).expect("fixture is valid JSON");
    let typed: T = serde_json::from_str(&text).unwrap_or_else(|error| panic!("{name} does not deserialise: {error}"));
    let again = serde_json::to_value(&typed).expect("serialises");
    assert_eq!(again, original, "{name} changed in a round trip");
    typed
}

#[test]
fn requests_round_trip() {
    let minimal: SolveRequest = round_trip("request_minimal.json");
    assert_eq!(minimal.format, Format::Lp);
    assert_eq!(minimal.diagnose, Diagnose::Auto);
    assert!(minimal.validate().is_ok());

    let full: SolveRequest = round_trip("request_full.json");
    assert_eq!(full.format, Format::Mps);
    assert_eq!(full.diagnose, Diagnose::Never);
    assert!(full.include_log);
    assert_eq!(full.options.time_limit, Some(30.0));
    assert_eq!(full.options.presolve, Some(Presolve::Off));
    assert_eq!(full.options.threads, Some(2));
}

#[test]
fn defaults_apply_when_fields_are_missing() {
    let request = SolveRequest::from_json(r#"{"protocol":1,"format":"lp","text":"x"}"#).expect("optional fields default");
    assert_eq!(request.diagnose, Diagnose::Auto);
    assert!(!request.include_log);
    assert_eq!(request.options, lp_highs::protocol::SolveOptions::default());
}

#[test]
fn responses_round_trip() {
    for name in ["response_optimal.json", "response_mip.json", "response_infeasible_iis.json", "response_infeasible_elastic.json"] {
        let response: SolveResponse = round_trip(name);
        assert!(response.ok, "{name}");
        assert!(response.validate().is_ok(), "{name}");
    }
    let error: SolveResponse = round_trip("response_error.json");
    assert!(!error.ok);
    assert_eq!(error.error.expect("error body").kind, ErrorKind::UnsupportedModel);
}

#[test]
fn mip_responses_omit_duals() {
    let response: SolveResponse = round_trip("response_mip.json");
    let result = response.result.expect("result");
    assert!(result.variables.iter().all(|variable| variable.reduced_cost.is_none()));
    assert!(result.constraints.iter().all(|constraint| constraint.dual.is_none()));
}

#[test]
fn events_round_trip() {
    let event: Event = round_trip("event_phase.json");
    assert_eq!(event, Event::Phase { phase: Phase::Building, elapsed_ms: 12 });
    assert_eq!(Event::from_line(&event.to_line()), Some(event));
    assert_eq!(Event::from_line("warning: something else"), None, "non-event stderr is not an event");
}

#[test]
fn a_foreign_protocol_version_is_rejected() {
    let error = SolveRequest::from_json(&fixture("request_wrong_protocol.json")).expect_err("protocol 2 must be refused");
    assert!(error.to_string().contains("unsupported protocol 2"), "{error}");
    let error = SolveResponse::from_json(&fixture("response_wrong_protocol.json")).expect_err("protocol 2 must be refused");
    assert!(error.to_string().contains("unsupported protocol 2"), "{error}");
}

#[test]
fn malformed_and_inconsistent_messages_are_rejected() {
    assert!(SolveRequest::from_json("not json").is_err());
    assert!(SolveRequest::from_json(r#"{"protocol":1,"format":"xml","text":""}"#).is_err(), "unknown format");
    assert!(SolveRequest::from_json(r#"{"protocol":1,"format":"lp","text":"","options":{"timeLimit":-1}}"#).is_err(), "negative limit");
    assert!(SolveResponse::from_json(r#"{"protocol":1,"ok":true}"#).is_err(), "ok without a result");
    assert!(SolveResponse::from_json(r#"{"protocol":1,"ok":false}"#).is_err(), "failure without an error");
}
