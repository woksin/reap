use super::ScanOpts;
use crate::model::{Action, Candidate, Category, Risk, ScanEvent};
use crate::util::human;
use serde::Deserialize;
use std::process::Command;
use std::sync::mpsc::Sender;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Df {
    #[serde(rename = "Images")]
    images: Vec<Image>,
    #[serde(rename = "Containers")]
    containers: Vec<Container>,
    #[serde(rename = "Volumes")]
    volumes: Vec<Volume>,
    #[serde(rename = "BuildCache")]
    build_cache: Vec<BuildCache>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Image {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Repository")]
    repository: String,
    #[serde(rename = "Tag")]
    tag: String,
    #[serde(rename = "UniqueSize")]
    unique_size: String,
    #[serde(rename = "Size")]
    size: String,
    #[serde(rename = "Containers")]
    containers: String,
    #[serde(rename = "CreatedSince")]
    created_since: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Container {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Names")]
    names: String,
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "State")]
    state: String,
    #[serde(rename = "Status")]
    status: String,
    #[serde(rename = "Size")]
    size: String,
    #[serde(rename = "CreatedSince")]
    created_since: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Volume {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Links")]
    links: String,
    #[serde(rename = "Size")]
    size: String,
    #[serde(rename = "Labels")]
    labels: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct BuildCache {
    #[serde(rename = "Size")]
    size: String,
    #[serde(rename = "InUse")]
    in_use: String,
    #[serde(rename = "Shared")]
    shared: String,
    #[serde(rename = "LastUsedSince")]
    last_used_since: String,
}

/// Docker reports sizes as display strings ("1.637GB", "73.7kB", "0B").
/// The daemon uses SI units here, so 1 GB is 1000^3.
fn parse_size(s: &str) -> u64 {
    let s = s.trim().trim_end_matches('*');
    if s.is_empty() || s == "N/A" {
        return 0;
    }
    let split = s
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let Ok(num) = num.trim().parse::<f64>() else {
        return 0;
    };
    let mult: f64 = match unit.trim() {
        "B" | "" => 1.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "KiB" => 1024.0,
        "MiB" => 1024f64.powi(2),
        "GiB" => 1024f64.powi(3),
        "TiB" => 1024f64.powi(4),
        _ => 1.0,
    };
    (num * mult) as u64
}

/// Turn docker's "4 days ago" into a day count.
fn parse_since(s: &str) -> Option<u64> {
    let s = s.trim().trim_end_matches(" ago").trim();
    let mut parts = s.split_whitespace();
    let head = parts.next()?;
    let unit = parts.next().unwrap_or("");
    let n: f64 = if head.eq_ignore_ascii_case("about") || head.eq_ignore_ascii_case("a") {
        1.0
    } else {
        head.parse().ok()?
    };
    let unit = if n == 1.0 && (head.eq_ignore_ascii_case("about") || head.eq_ignore_ascii_case("a"))
    {
        parts.next().unwrap_or(unit)
    } else {
        unit
    };
    let days = match unit.trim_end_matches('s') {
        "second" | "minute" | "hour" => 0.0,
        "day" => n,
        "week" => n * 7.0,
        "month" => n * 30.0,
        "year" => n * 365.0,
        _ => return None,
    };
    Some(days as u64)
}

pub fn scan(opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let _ = tx.send(ScanEvent::Status("docker: querying daemon".into()));
    let Ok(out) = Command::new("docker")
        .args(["system", "df", "-v", "--format", "{{json .}}"])
        .output()
    else {
        return; // docker not installed
    };
    if !out.status.success() {
        // Daemon down, or no permission. Silently contribute nothing.
        return;
    }
    let Ok(df) = serde_json::from_slice::<Df>(&out.stdout) else {
        return;
    };

    images(&df, opts, tx);
    containers(&df, tx);
    volumes(&df, tx);
    build_cache(&df, tx);
    networks(tx);
}

fn images(df: &Df, opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    for img in &df.images {
        // An image backing a live container is not stale.
        if img.containers.parse::<u32>().unwrap_or(0) > 0 {
            continue;
        }
        let dangling = img.repository == "<none>" || img.repository.is_empty();
        let age = parse_since(&img.created_since);

        // UniqueSize is what actually comes back; the rest is shared with
        // other images and stays put.
        let unique = parse_size(&img.unique_size);
        let total = parse_size(&img.size);

        let (group, risk) = if dangling {
            ("dangling images", Risk::Safe)
        } else {
            ("unused images", Risk::Caution)
        };

        let name = if dangling {
            format!("<none>  {}", short_id(&img.id))
        } else {
            format!("{}:{}", img.repository, img.tag)
        };

        // Removing by tag is gentler than by ID: an image carrying several tags
        // only loses the one we name.
        let target = if dangling {
            short_id(&img.id).to_string()
        } else {
            format!("{}:{}", img.repository, img.tag)
        };

        let detail = format!(
            "no containers · {} total, {} shared with other images",
            human(total),
            human(total.saturating_sub(unique))
        );

        let cand = Candidate::new(
            Category::Docker,
            group,
            name,
            detail,
            unique,
            risk,
            Action::Run {
                program: "docker".into(),
                args: vec!["rmi".into(), target],
                cwd: None,
            },
        )
        .with_age(age);

        if unique >= opts.min_size || dangling {
            let _ = tx.send(ScanEvent::Found(Box::new(cand)));
        }
    }
}

fn containers(df: &Df, tx: &Sender<ScanEvent>) {
    for c in &df.containers {
        if c.state == "running" || c.state == "restarting" || c.state == "paused" {
            continue;
        }
        let cand = Candidate::new(
            Category::Docker,
            "stopped containers",
            c.names.clone(),
            format!("{} · {} · from {}", c.state, c.status, c.image),
            parse_size(&c.size),
            Risk::Caution,
            Action::Run {
                program: "docker".into(),
                args: vec!["rm".into(), short_id(&c.id).to_string()],
                cwd: None,
            },
        )
        .with_age(parse_since(&c.created_since));
        let _ = tx.send(ScanEvent::Found(Box::new(cand)));
    }
}

fn volumes(df: &Df, tx: &Sender<ScanEvent>) {
    for v in &df.volumes {
        // Links counts containers holding the volume.
        if v.links.parse::<u32>().unwrap_or(0) > 0 {
            continue;
        }
        let anonymous = v.labels.contains("com.docker.volume.anonymous")
            || (v.name.len() == 64 && v.name.chars().all(|c| c.is_ascii_hexdigit()));

        let (group, name, detail) = if anonymous {
            (
                "anonymous volumes",
                format!("anon  {}", &v.name[..v.name.len().min(12)]),
                "unreferenced, left behind by a removed container".to_string(),
            )
        } else {
            (
                "unused volumes",
                v.name.clone(),
                "named volume with no container attached".to_string(),
            )
        };

        // Volumes are the one place real data hides, so they are never "safe".
        let cand = Candidate::new(
            Category::Docker,
            group,
            name,
            detail,
            parse_size(&v.size),
            Risk::Danger,
            Action::Run {
                program: "docker".into(),
                args: vec!["volume".into(), "rm".into(), v.name.clone()],
                cwd: None,
            },
        );
        let _ = tx.send(ScanEvent::Found(Box::new(cand)));
    }
}

fn build_cache(df: &Df, tx: &Sender<ScanEvent>) {
    // BuildKit records cannot be pruned individually by ID, so the whole
    // reclaimable set is offered as one item — which is also the single
    // biggest win on most machines.
    let reclaimable: Vec<&BuildCache> = df
        .build_cache
        .iter()
        .filter(|b| b.in_use == "false" && b.shared == "false")
        .collect();
    if reclaimable.is_empty() {
        return;
    }
    let total: u64 = reclaimable.iter().map(|b| parse_size(&b.size)).sum();
    if total == 0 {
        return;
    }
    let oldest = reclaimable
        .iter()
        .filter_map(|b| parse_since(&b.last_used_since))
        .max();

    let cand = Candidate::new(
        Category::Docker,
        "build cache",
        "BuildKit cache (all reclaimable)",
        format!(
            "{} unused layer records · rebuilds will be slower once, then re-cache",
            reclaimable.len()
        ),
        total,
        Risk::Safe,
        Action::Run {
            program: "docker".into(),
            args: vec![
                "builder".into(),
                "prune".into(),
                "--all".into(),
                "--force".into(),
            ],
            cwd: None,
        },
    )
    .with_age(oldest);
    let _ = tx.send(ScanEvent::Found(Box::new(cand)));
}

fn networks(tx: &Sender<ScanEvent>) {
    let Ok(out) = Command::new("docker")
        .args(["network", "ls", "--filter", "dangling=true", "--format", "{{.Name}}"])
        .output()
    else {
        return;
    };
    let names: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        // The three built-in networks are always reported as dangling.
        .filter(|l| !l.is_empty() && !matches!(l.as_str(), "bridge" | "host" | "none"))
        .collect();
    if names.is_empty() {
        return;
    }
    let cand = Candidate::new(
        Category::Docker,
        "networks",
        format!("{} unused networks", names.len()),
        "no containers attached · frees bridge interfaces, not disk".to_string(),
        0,
        Risk::Safe,
        Action::Run {
            program: "docker".into(),
            args: vec!["network".into(), "prune".into(), "--force".into()],
            cwd: None,
        },
    );
    let _ = tx.send(ScanEvent::Found(Box::new(cand)));
}

fn short_id(id: &str) -> &str {
    let id = id.strip_prefix("sha256:").unwrap_or(id);
    &id[..id.len().min(12)]
}
