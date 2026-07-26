//! Which directories reap is willing to call build output.
//!
//! The whole safety of this category rests on evidence: a directory is only
//! offered when a sibling file proves what produced it. Without that, a source
//! directory that happens to be called `build` would be deleted.

use super::given::a_project::a_project;
use crate::config::{ArtifactRule, Config, RiskName};
use crate::model::Candidate;
use std::sync::LazyLock;

fn labels(candidates: &[Candidate]) -> Vec<&str> {
    candidates.iter().map(|c| c.label.as_str()).collect()
}

mod when_a_build_directory_sits_beside_its_manifest {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_project::new()
            .with_a_file("Cargo.toml")
            .with_a_directory("target")
            .candidates()
    });

    #[test]
    fn should_offer_it() {
        assert!(
            labels(&BECAUSE).iter().any(|l| l.ends_with("/target")),
            "found: {:?}",
            labels(&BECAUSE)
        );
    }

    #[test]
    fn should_say_what_would_rebuild_it() {
        let cand = BECAUSE
            .iter()
            .find(|c| c.label.ends_with("/target"))
            .unwrap();
        assert!(
            cand.detail.contains("cargo build"),
            "detail was: {}",
            cand.detail
        );
    }

    #[test]
    fn should_measure_what_it_holds() {
        let cand = BECAUSE
            .iter()
            .find(|c| c.label.ends_with("/target"))
            .unwrap();
        assert_eq!(cand.size, 2048);
    }
}

mod when_a_directory_only_shares_the_name_of_build_output {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        // No Cargo.toml, so nothing proves this `target` was ever built.
        a_project::new().with_a_directory("target").candidates()
    });

    #[test]
    fn should_leave_it_alone() {
        assert!(
            BECAUSE.is_empty(),
            "a directory with no evidence must not be offered: {:?}",
            labels(&BECAUSE)
        );
    }
}

mod when_evidence_is_matched_by_extension {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        // .NET proves itself with a project file whose name varies.
        a_project::new()
            .with_a_file("Whatever.csproj")
            .with_a_directory("bin")
            .with_a_directory("obj")
            .candidates()
    });

    #[test]
    fn should_offer_every_directory_that_project_file_accounts_for() {
        let found = labels(&BECAUSE);
        assert!(
            found.iter().any(|l| l.ends_with("/bin")),
            "found: {found:?}"
        );
        assert!(
            found.iter().any(|l| l.ends_with("/obj")),
            "found: {found:?}"
        );
    }
}

mod when_a_directory_needs_no_evidence_at_all {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        // Nothing but a Python interpreter creates __pycache__, so its name is
        // proof enough.
        a_project::new()
            .with_a_directory("__pycache__")
            .candidates()
    });

    #[test]
    fn should_offer_it_on_its_name_alone() {
        assert!(
            labels(&BECAUSE).iter().any(|l| l.ends_with("/__pycache__")),
            "found: {:?}",
            labels(&BECAUSE)
        );
    }
}

mod when_build_output_is_nested_inside_other_build_output {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_project::new()
            .with_a_file("package.json")
            .with_a_directory("node_modules")
            .with_a_directory("node_modules/some-package/node_modules")
            .candidates()
    });

    #[test]
    fn should_report_only_the_outermost_one() {
        // Counting the inner copy too would promise the same bytes twice.
        assert_eq!(
            BECAUSE.len(),
            1,
            "expected one candidate, found: {:?}",
            labels(&BECAUSE)
        );
    }
}

mod when_the_configuration_adds_a_rule {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        let mut cfg = Config::default();
        cfg.artifacts.push(ArtifactRule {
            dir: "my-output".into(),
            evidence: vec!["Makefile".into()],
            regen: "make".into(),
            risk: RiskName::Safe,
        });
        a_project::new()
            .with_a_file("Makefile")
            .with_a_directory("my-output")
            .candidates_with(cfg)
    });

    #[test]
    fn should_offer_the_directory_that_rule_describes() {
        assert!(
            labels(&BECAUSE).iter().any(|l| l.ends_with("/my-output")),
            "found: {:?}",
            labels(&BECAUSE)
        );
    }

    #[test]
    fn should_apply_the_risk_the_rule_declares() {
        let cand = BECAUSE
            .iter()
            .find(|c| c.label.ends_with("/my-output"))
            .unwrap();
        assert_eq!(cand.risk, crate::model::Risk::Safe);
    }

    #[test]
    fn should_still_require_that_rules_evidence() {
        let mut cfg = Config::default();
        cfg.artifacts.push(ArtifactRule {
            dir: "my-output".into(),
            evidence: vec!["Makefile".into()],
            regen: "make".into(),
            risk: RiskName::Safe,
        });
        // Same rule, but nothing proves this directory is its output.
        let without_evidence = a_project::new()
            .with_a_directory("my-output")
            .candidates_with(cfg);
        assert!(
            without_evidence.is_empty(),
            "found: {:?}",
            labels(&without_evidence)
        );
    }
}

mod when_the_configuration_ignores_a_path {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        let mut cfg = Config::default();
        cfg.ignore.push("*/node_modules".into());
        a_project::new()
            .with_a_file("package.json")
            .with_a_directory("node_modules")
            .with_a_file("Cargo.toml")
            .with_a_directory("target")
            .candidates_with(cfg)
    });

    #[test]
    fn should_withhold_what_the_pattern_matches() {
        assert!(
            !labels(&BECAUSE)
                .iter()
                .any(|l| l.ends_with("/node_modules")),
            "found: {:?}",
            labels(&BECAUSE)
        );
    }

    #[test]
    fn should_still_offer_everything_else() {
        assert!(
            labels(&BECAUSE).iter().any(|l| l.ends_with("/target")),
            "found: {:?}",
            labels(&BECAUSE)
        );
    }
}
