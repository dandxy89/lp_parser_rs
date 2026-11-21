//! `lp-highs`: the `HiGHS` helper process (protocol in [`lp_highs::protocol`]).
//!
//! * `lp-highs info`: one JSON object describing the helper, exit 0.
//! * `lp-highs --version`: `lp-highs <version>`.
//! * `lp-highs solve`: one request on stdin, one response on stdout, progress
//!   as NDJSON on stderr. Exit 0 whenever a response was written (infeasible
//!   included), 2 for usage and protocol errors, 1 for internal failure.

use std::io::{Read, Write};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use lp_highs::protocol::{ErrorKind, HelperInfo, MAX_REQUEST_BYTES, PROTOCOL_VERSION, SolveRequest, SolveResponse};

const EXIT_INTERNAL: u8 = 1;
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--version"] => {
            println!("lp-highs {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        ["info"] => info(),
        ["solve"] => std::panic::catch_unwind(solve).unwrap_or_else(|_| {
            eprintln!("lp-highs: internal failure (panic)");
            ExitCode::from(EXIT_INTERNAL)
        }),
        _ => {
            eprintln!("usage: lp-highs <info | solve | --version>");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

fn info() -> ExitCode {
    let info = HelperInfo {
        name: "lp-highs".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol: PROTOCOL_VERSION,
        highs: lp_highs::highs_version(),
    };
    match serde_json::to_string(&info) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("lp-highs: cannot serialise info: {error}");
            ExitCode::from(EXIT_INTERNAL)
        }
    }
}

/// Read stdin, at most [`MAX_REQUEST_BYTES`].
fn read_request() -> Result<String, String> {
    let mut input = String::new();
    let limit = u64::try_from(MAX_REQUEST_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    std::io::stdin().lock().take(limit).read_to_string(&mut input).map_err(|error| format!("cannot read stdin: {error}"))?;
    if input.len() > MAX_REQUEST_BYTES {
        return Err(format!("request exceeds {MAX_REQUEST_BYTES} bytes"));
    }
    Ok(input)
}

fn write_response(response: &SolveResponse) -> Result<(), String> {
    let json = serde_json::to_string(response).map_err(|error| format!("cannot serialise response: {error}"))?;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{json}").and_then(|()| stdout.flush()).map_err(|error| format!("cannot write response: {error}"))
}

fn solve() -> ExitCode {
    let request = read_request().and_then(|input| SolveRequest::from_json(&input).map_err(|violation| violation.to_string()));
    let request = match request {
        Ok(request) => request,
        Err(message) => {
            eprintln!("lp-highs: {message}");
            // Best effort: the client may already have gone, in which case the
            // exit status is all it can read.
            if let Err(error) = write_response(&SolveResponse::failure(ErrorKind::Protocol, message)) {
                eprintln!("lp-highs: {error}");
            }
            return ExitCode::from(EXIT_USAGE);
        }
    };

    // Cancellation is by killing the process; the flag is never set here.
    let cancel = AtomicBool::new(false);
    // Progress is advisory: if stderr closes, stop reporting but keep solving.
    let mut progress_open = true;
    let response = lp_highs::run_request(&request, &cancel, |event| {
        if progress_open && writeln!(std::io::stderr().lock(), "{}", event.to_line()).is_err() {
            progress_open = false;
        }
    });

    match write_response(&response) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("lp-highs: {message}");
            ExitCode::from(EXIT_INTERNAL)
        }
    }
}
