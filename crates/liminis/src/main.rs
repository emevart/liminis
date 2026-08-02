//! The `liminis` binary.
//!
//! Two commands. `run` reads a scenario config, applies defaults, and prints the
//! identity of the run it would have performed; there is no tick loop yet.
//! `serve` puts the viewer on a local port — the page is complete, and the
//! routes that would feed it data answer honestly that there is no world behind
//! them.

mod http;

use anyhow::Result;
use clap::{Parser, Subcommand};
use http::{Method, Request, Response};
use liminis_core::{config, version::WORLD_FORMAT_VERSION};
use std::net::TcpListener;
use std::path::PathBuf;

/// The viewer, compiled in rather than read from disk.
///
/// One binary and one command, which is what open question C-5 asks for. It also
/// means this server has no path that reaches the filesystem at all — see the
/// note at the top of `http.rs`.
const VIEWER: &str = include_str!("viewer.html");

#[derive(Debug, Parser)]
#[command(
    name = "liminis",
    version,
    about = "Voxel simulator of a biological ecosystem"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Read a scenario config and print the run identity.
    Run {
        /// Path to a scenario TOML file.
        #[arg(long, value_name = "PATH")]
        config: PathBuf,
        /// Seed for the run.
        #[arg(long, value_name = "N")]
        seed: u64,
        /// Print the canonical form the hash was taken over, and exit.
        ///
        /// The one thing that makes the roster of ADR-065 visible. Since a
        /// missing `[[process]]` record means the process's default rather than
        /// its absence, the configuration a scenario describes and the text of the
        /// scenario are no longer the same document: nine records nobody wrote
        /// stand between them. ADR-065 says so outright — "without it the user who
        /// forgot a record sees nothing at all."
        #[arg(long)]
        print_canonical: bool,
    },
    /// Serve the viewer on a local port.
    ///
    /// The page is finished; the world it wants to show is not. Every data
    /// route answers 503 and names what is missing, which is what the viewer
    /// renders as "lost the simulator".
    Serve {
        /// Port to listen on.
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Run {
            config,
            seed,
            print_canonical,
        } => {
            let scenario = config::load(&config)?;
            if print_canonical {
                // The exact bytes the hash is taken over, and not a re-rendering
                // of them: `config::canonical` is what `config_hash` consumes, so
                // what is printed here cannot drift away from what is hashed.
                print!("{}", config::canonical(&scenario)?);
                return Ok(());
            }
            let hash = config::config_hash(&scenario)?;
            println!(
                "seed={seed} config_hash={hash} world_format_version={WORLD_FORMAT_VERSION} code_version={}",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        }
        Command::Serve { port } => serve(port),
    }
}

fn serve(port: u16) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    println!("liminis viewer on http://127.0.0.1:{port}/");
    println!(
        "no world behind it yet: the data routes answer 503 until the tick loop \
         exists (docs/superpowers/plans/2026-08-01-s0-core.md, wave 6)"
    );
    http::serve(listener, route)?;
    Ok(())
}

/// The four routes of the viewer contract, and what each can answer today.
///
/// The three data routes refuse rather than fabricate. A viewer fed plausible
/// numbers is worse than a viewer fed nothing: this project exists to tell an
/// interesting result from a bug, and a picture of invented data is a bug that
/// looks like a result.
fn route(request: &Request) -> Response {
    let path = request.path.as_str();

    if path == "/" || path == "/index.html" {
        return match request.method {
            Method::Get => Response::html(VIEWER),
            _ => Response::error(405, "the viewer is a GET"),
        };
    }

    if path == "/api/control" {
        return match request.method {
            Method::Post => Response::error(503, MISSING),
            _ => Response::error(405, "control is a POST"),
        };
    }

    if path == "/api/state" || path.starts_with("/api/volume/") || path.starts_with("/api/profile/")
    {
        return match request.method {
            Method::Get => Response::error(503, MISSING),
            _ => Response::error(405, "the data routes are GETs"),
        };
    }

    Response::error(404, "no such route")
}

const MISSING: &str = "no world yet: liminis has a grid, fields and one kernel, and no tick loop \
     to run them. See docs/superpowers/plans/2026-08-01-s0-core.md.";

#[cfg(test)]
mod tests {
    use super::*;

    fn get(path: &str) -> Response {
        route(&Request {
            method: Method::Get,
            path: path.into(),
            query: String::new(),
            body: Vec::new(),
        })
    }

    #[test]
    fn the_viewer_is_served_whole() {
        let response = get("/");
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "text/html; charset=utf-8");
        // Compiled in, so this is the same page the binary will always serve —
        // there is no file to go missing between build and run.
        assert!(response.body.starts_with(b"<!doctype html>"));
        assert!(response.body.len() > 10_000);
    }

    #[test]
    fn a_data_route_refuses_instead_of_inventing() {
        // The one thing this server must not do before the world exists. A
        // viewer showing plausible numbers is indistinguishable from a viewer
        // showing a result, and this project is built to tell those apart.
        for path in ["/api/state", "/api/volume/O2", "/api/profile/H2S"] {
            let response = get(path);
            assert_eq!(response.status, 503, "{path} answered something");
            let body = String::from_utf8(response.body).unwrap();
            assert!(
                body.contains("no world yet"),
                "{path} was unhelpful: {body}"
            );
        }
    }

    #[test]
    fn an_unknown_route_is_a_404_and_not_the_viewer() {
        assert_eq!(get("/api/nothing").status, 404);
        assert_eq!(get("/etc/passwd").status, 404);
        assert_eq!(get("/../../etc/passwd").status, 404);
    }

    #[test]
    fn the_wrong_method_is_a_405_that_says_which_one_was_wanted() {
        let post = |path: &str| {
            route(&Request {
                method: Method::Post,
                path: path.into(),
                query: String::new(),
                body: Vec::new(),
            })
        };
        assert_eq!(post("/").status, 405);
        assert_eq!(post("/api/state").status, 405);
        assert_eq!(get("/api/control").status, 405);
    }
}
