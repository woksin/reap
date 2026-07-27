//! What a docker daemon reported, as reap actually receives it.
//!
//! The other fixtures build the real thing — real repositories, real
//! directories — because the behaviour under specification is what git and the
//! filesystem say. Images and volumes cannot be conjured that way inside a
//! test, so the equivalent here is docker's own output: `docker system df -v
//! --format '{{json .}}'`, captured from a running daemon and sanitised of
//! names, kept verbatim in shape and in every figure's spelling.
//!
//! That spelling is the point. `parse_size` reads display strings, so a
//! fixture that invented its own would specify nothing about the strings
//! docker really emits.

use super::scanning_everything;
use crate::model::{Candidate, ScanEvent};
use serde_json::{Value, json};

/// A capture from a real daemon: five images (one backing a live container,
/// one dangling), three containers, three volumes, four build cache records.
pub const CAPTURED: &str = include_str!("docker_system_df.json");

/// A daemon whose reported state the specification composes.
///
/// Starts from the capture, so every record carries the full field set docker
/// emits rather than only the fields reap happens to read.
pub struct a_docker_daemon {
    df: Value,
}

impl a_docker_daemon {
    /// Exactly what the capture holds.
    pub fn reporting_what_was_captured() -> Self {
        Self {
            df: serde_json::from_str(CAPTURED).expect("the captured output parses"),
        }
    }

    /// Whatever a daemon actually said, for cross-checking against a live one.
    pub fn reporting_exactly(df: Value) -> Self {
        Self { df }
    }

    /// A daemon with nothing on it, for scenarios that add one record and
    /// observe only that.
    pub fn reporting_nothing() -> Self {
        Self {
            df: json!({ "Images": [], "Containers": [], "Volumes": [], "BuildCache": [] }),
        }
    }

    /// An image no container is using, sized as docker would state it.
    pub fn with_an_unused_image(mut self, repo_tag: &str, unique: &str, total: &str) -> Self {
        let (repository, tag) = repo_tag.split_once(':').unwrap_or((repo_tag, "latest"));
        self.push(
            "Images",
            json!({
                "Containers": "0",
                "CreatedSince": "3 days ago",
                "ID": format!("sha256:{:0>64}", repository),
                "Repository": repository,
                "Tag": tag,
                "Size": total,
                "SharedSize": "0B",
                "UniqueSize": unique,
            }),
        );
        self
    }

    /// A container in the given state — `running`, `exited`, `created`.
    pub fn with_a_container(mut self, name: &str, state: &str, size: &str) -> Self {
        self.push(
            "Containers",
            json!({
                "ID": format!("{:0>64}", name),
                "Names": name,
                "Image": "some/image:latest",
                "State": state,
                "Status": "Exited (0) 3 days ago",
                "Size": size,
                "CreatedSince": "3 days ago",
            }),
        );
        self
    }

    /// A volume, attached to `links` containers.
    pub fn with_a_volume(mut self, name: &str, links: u32, size: &str) -> Self {
        self.push(
            "Volumes",
            json!({
                "Name": name,
                "Links": links.to_string(),
                "Size": size,
                "Labels": "",
                "Driver": "local",
                "Scope": "local",
            }),
        );
        self
    }

    /// A volume docker left unnamed when the container that made it went away.
    pub fn with_an_anonymous_volume(mut self, size: &str) -> Self {
        self.push(
            "Volumes",
            json!({
                "Name": "f".repeat(64),
                "Links": "0",
                "Size": size,
                "Labels": "com.docker.volume.anonymous=",
                "Driver": "local",
                "Scope": "local",
            }),
        );
        self
    }

    /// One BuildKit layer record.
    pub fn with_a_build_cache_record(mut self, size: &str, in_use: bool, shared: bool) -> Self {
        self.push(
            "BuildCache",
            json!({
                "ID": "cacherecord",
                "Size": size,
                "InUse": in_use.to_string(),
                "Shared": shared.to_string(),
                "LastUsedSince": "5 days ago",
                "CreatedSince": "5 days ago",
                "UsageCount": "1",
            }),
        );
        self
    }

    fn push(&mut self, key: &str, record: Value) {
        self.df[key]
            .as_array_mut()
            .expect("a list of records")
            .push(record);
    }

    /// Everything the docker scan reports for this daemon.
    pub fn candidates(self) -> Vec<Candidate> {
        self.candidates_with(crate::config::Config::default())
    }

    /// The same, under a given configuration.
    pub fn candidates_with(self, cfg: crate::config::Config) -> Vec<Candidate> {
        self.candidates_under(cfg, 0)
    }

    /// The same, with a floor under what is worth reporting — the one option
    /// this scanner consults, and the reason an unreadable size must not be
    /// treated as a small one.
    pub fn candidates_above(self, min_size: u64) -> Vec<Candidate> {
        self.candidates_under(crate::config::Config::default(), min_size)
    }

    fn candidates_under(self, cfg: crate::config::Config, min_size: u64) -> Vec<Candidate> {
        let mut opts = scanning_everything(vec![]);
        opts.rules = std::sync::Arc::new(crate::scan::Rules::from_config(&cfg));
        opts.min_size = min_size;

        let (tx, rx) = std::sync::mpsc::channel();
        crate::scan::docker::candidates_from_df(self.df.to_string().as_bytes(), &opts, &tx);
        drop(tx);

        rx.into_iter()
            .filter_map(|e| match e {
                ScanEvent::Found(c) => Some(*c),
                _ => None,
            })
            .collect()
    }
}
