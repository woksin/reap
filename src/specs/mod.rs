//! Behavioural specifications.
//!
//! `for_<subject>` names what is under specification, `when_<scenario>` names
//! the situation, and each `should_<expectation>` observes exactly one thing —
//! so a failure reads as a sentence and names precisely what broke.
//!
//! Each scenario establishes its context through `given`, performs the act once
//! in `BECAUSE`, and only observes thereafter. Where the unit tests beside the
//! code check mechanics, these specify behaviour: they drive the real scanners
//! against real git repositories and real directory trees, and assert on what
//! a user would actually be shown.

// Contexts are named as sentence fragments — `a_repository`, `a_project` —
// which is the whole point of reading a spec aloud. Rust's camel-case rule
// works against that here and nowhere else.
#![allow(non_camel_case_types)]

mod given;

mod for_agent_triage;
mod for_artifact_detection;
mod for_branch_prunability;
mod for_configuration_editing;
mod for_docker_reclaim;
mod for_personal_triage;
mod for_size_reporting;
mod for_worktree_prunability;
