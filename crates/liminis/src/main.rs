//! The `liminis` binary.
//!
//! Two commands. `run` reads a scenario config, applies defaults, and prints the
//! identity of the run it would have performed. `serve` builds a world out of
//! that same config, runs it in a thread of its own and puts the viewer on a
//! local port — the page and the four routes behind it are the two halves of one
//! contract (ADR-070), and the half that turns a world into bytes lives in
//! `serve.rs`.

mod http;
#[cfg(test)]
mod json;
mod serve;

use anyhow::Result;
use clap::{Parser, Subcommand};
use http::{Method, Request, Response};
use liminis_core::{config, version::WORLD_FORMAT_VERSION};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// The viewer, compiled in rather than read from disk.
///
/// One binary and one command, which is what open question C-5 asks for. It also
/// means no URL selects a disk file. Experiment storage is controlled by the
/// host, not by this router — see the note at the top of `http.rs`.
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
    /// Run a scenario and serve the viewer on a local port.
    ///
    /// Defaults to the small genetic-colony scenario; config and seed remain
    /// explicit overrides for reproducible experiments (ADR-095).
    Serve {
        /// Path to a scenario TOML file.
        #[arg(long, value_name = "PATH", conflicts_with = "resume")]
        config: Option<PathBuf>,
        /// Seed for the run.
        #[arg(long, value_name = "N", conflicts_with = "resume")]
        seed: Option<u64>,
        /// Port to listen on.
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Local experiment storage (outside build outputs).
        #[arg(long, default_value = ".liminis/runs", value_name = "PATH")]
        data_dir: PathBuf,
        /// Continue a saved experiment, using its exact config and seed.
        #[arg(long, value_name = "RUN_ID|latest")]
        resume: Option<String>,
        /// Explicit older checkpoint; never silently fall back from a corrupt latest.
        #[arg(long, value_name = "CHECKPOINT_ID", requires = "resume")]
        checkpoint: Option<String>,
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
        Command::Serve {
            config,
            seed,
            port,
            data_dir,
            resume,
            checkpoint,
        } => serve::run(
            port,
            &config.unwrap_or_else(|| PathBuf::from("configs/scenarios/genetic-colony.toml")),
            seed.unwrap_or(42),
            &data_dir,
            resume.as_deref(),
            checkpoint.as_deref(),
        ),
    }
}

/// The routes of the binary: the page, the method policy, and the 404.
///
/// The four data routes are `serve.rs`'s and are delegated whole. What stays
/// here is what belongs to the binary rather than to the world — the viewer
/// itself, the answer to a method nobody asked for, and the refusal of a path
/// that is not a route at all.
fn route(shared: &Arc<Mutex<serve::Sim>>, request: &Request) -> Response {
    let path = request.path.as_str();

    if path == "/" || path == "/index.html" {
        return match request.method {
            Method::Get => Response::html(VIEWER),
            _ => Response::error(405, "the viewer is a GET"),
        };
    }

    if path == "/api/control" {
        return match request.method {
            Method::Post => serve::route(shared, request),
            _ => Response::error(405, "control is a POST"),
        };
    }

    if path == "/api/state"
        || path == "/api/ecology"
        || path == "/api/history"
        || path.starts_with("/api/volume/")
        || path.starts_with("/api/profile/")
    {
        return match request.method {
            Method::Get => serve::route(shared, request),
            _ => Response::error(405, "the data routes are GETs"),
        };
    }

    Response::error(404, "no such route")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serve::fixture;

    #[test]
    fn serve_defaults_to_the_accepted_genetic_experiment() {
        let Command::Serve {
            config,
            seed,
            port,
            data_dir,
            resume,
            checkpoint,
        } = Cli::try_parse_from(["liminis", "serve"]).unwrap().command
        else {
            panic!("serve must select the local observer");
        };
        assert_eq!(config, None);
        assert_eq!(seed, None);
        assert_eq!(port, 8080);
        assert_eq!(data_dir, PathBuf::from(".liminis/runs"));
        assert!(resume.is_none() && checkpoint.is_none());
    }

    #[test]
    fn resume_refuses_conflicting_identity_and_checkpoint_without_resume() {
        for args in [
            vec!["liminis", "serve", "--resume", "latest", "--seed", "42"],
            vec![
                "liminis", "serve", "--resume", "latest", "--config", "foo.toml",
            ],
            vec!["liminis", "serve", "--checkpoint", "checkpoint-1"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    fn get(shared: &Arc<Mutex<serve::Sim>>, path: &str) -> Response {
        route(
            shared,
            &Request {
                method: Method::Get,
                path: path.into(),
                query: String::new(),
                body: Vec::new(),
            },
        )
    }

    #[test]
    fn the_viewer_is_served_whole() {
        let shared = fixture::sim();
        let response = get(&shared, "/");
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "text/html; charset=utf-8");
        // Compiled in, so this is the same page the binary will always serve —
        // there is no file to go missing between build and run.
        assert!(response.body.starts_with(b"<!doctype html>"));
        assert!(response.body.len() > 10_000);
    }

    #[test]
    fn the_four_routes_are_the_ones_the_viewer_asks_for() {
        // The replacement for `a_data_route_refuses_instead_of_inventing`, which
        // went out with the 503 it was written against. A test left standing on
        // a branch nobody takes is green for ever and says nothing.
        let shared = fixture::sim();
        for path in ["/api/state", "/api/volume/O2", "/api/profile/O2"] {
            let response = get(&shared, path);
            assert_eq!(response.status, 200, "{path} did not answer from the world");
            assert!(!response.body.is_empty(), "{path} answered with nothing");
        }

        let response = route(
            &shared,
            &Request {
                method: Method::Post,
                path: "/api/control".into(),
                query: String::new(),
                body: b"{\"action\":\"pause\"}".to_vec(),
            },
        );
        assert_eq!(response.status, 200);
    }

    #[test]
    fn an_unknown_route_is_a_404_and_not_the_viewer() {
        let shared = fixture::sim();
        assert_eq!(get(&shared, "/api/nothing").status, 404);
        assert_eq!(get(&shared, "/etc/passwd").status, 404);
        assert_eq!(get(&shared, "/../../etc/passwd").status, 404);
    }

    #[test]
    fn the_wrong_method_is_a_405_that_says_which_one_was_wanted() {
        let shared = fixture::sim();
        let post = |path: &str| {
            route(
                &shared,
                &Request {
                    method: Method::Post,
                    path: path.into(),
                    query: String::new(),
                    body: Vec::new(),
                },
            )
        };
        assert_eq!(post("/").status, 405);
        assert_eq!(post("/api/state").status, 405);
        assert_eq!(get(&shared, "/api/control").status, 405);
    }
}
