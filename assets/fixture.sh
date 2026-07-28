#!/usr/bin/env bash
#
# Builds a throwaway set of repositories for the git recording.
#
#   ./assets/fixture.sh            # into /tmp/reap-demo
#   ./assets/fixture.sh /some/dir
#
# The README's other recordings film this machine as it actually is. This one
# cannot: judging a branch means naming it, and every repository on the machine
# that made these belongs to someone who did not agree to appear in a README.
#
# So the branches here are invented, but nothing else is. Each repository is a
# real repository with a real bare remote beside it, and every verdict reap
# reaches is reached the same way it would be on yours — `git cherry` against a
# genuine upstream, not a fixture pretending to have one. A branch called
# irreversible here is irreversible because its commit exists in exactly one
# place, which is a fact about this directory and not a label.
#
# Commits are backdated so the ages read like a repository somebody has been
# living in rather than one created ninety seconds ago.

set -euo pipefail

ROOT="${1:-/tmp/reap-demo}"
rm -rf "$ROOT"
mkdir -p "$ROOT"

# Dates are relative to now so the fixture never goes stale, and reap's default
# is to ignore anything touched in the last 30 days.
ago() { date -v-"$1"d +"%Y-%m-%dT%H:%M:%S" 2>/dev/null || date -d "$1 days ago" +"%Y-%m-%dT%H:%M:%S"; }

git_at() {
  local days=$1 dir=$2
  shift 2
  GIT_AUTHOR_DATE="$(ago "$days")" GIT_COMMITTER_DATE="$(ago "$days")" \
    git -C "$dir" -c user.name="Demo" -c user.email="demo@example.invalid" \
        -c commit.gpgsign=false -c advice.detachedHead=false "$@" >/dev/null 2>&1
}

# A working repository with a bare origin beside it. The remote is not scenery:
# whether a branch is recoverable turns entirely on what a remote can reach.
new_repo() {
  local name=$1
  local work="$ROOT/$name" origin="$ROOT/.remotes/$name.git"
  mkdir -p "$work" "$origin"
  git -C "$origin" init --bare -b main >/dev/null
  git -C "$work" init -b main >/dev/null
  git -C "$work" remote add origin "$origin"
  echo "# $name" > "$work/README.md"
  git_at 400 "$work" add README.md
  git_at 400 "$work" commit -m "initial"
  git_at 400 "$work" push -u origin main
}

commit_on() {
  local days=$1 work=$2 branch=$3
  git_at "$days" "$work" checkout -q -b "$branch"
  echo "$branch" > "$work/${branch//\//-}.txt"
  git_at "$days" "$work" add -A
  git_at "$days" "$work" commit -m "${branch##*/}"
  git_at "$days" "$work" checkout -q main
}

# Reachable from main: nothing is lost by deleting it.
merged() {
  local days=$1 work=$ROOT/$2 branch=$3
  commit_on "$days" "$work" "$branch"
  git_at "$days" "$work" merge --no-ff -m "merge $branch" "$branch"
  git_at "$days" "$work" push origin main
}

# In main by content but not an ancestor of it — what a squash-merged pull
# request leaves behind, and the case `git branch --merged` gets wrong.
squash_merged() {
  local days=$1 work=$ROOT/$2 branch=$3
  commit_on "$days" "$work" "$branch"
  git_at "$days" "$work" merge --squash "$branch"
  git_at "$days" "$work" commit -m "squash $branch"
  git_at "$days" "$work" push origin main
}

# Unmerged, but every commit is on the remote and can be fetched back.
pushed() {
  local days=$1 work=$ROOT/$2 branch=$3
  commit_on "$days" "$work" "$branch"
  git_at "$days" "$work" push -u origin "$branch"
}

# Commits that exist in this clone and nowhere else.
unpushed() {
  local days=$1 work=$ROOT/$2 branch=$3
  commit_on "$days" "$work" "$branch"
}

# Pushed, then deleted upstream while still holding local commits.
orphaned() {
  local days=$1 work=$ROOT/$2 branch=$3
  commit_on "$days" "$work" "$branch"
  git_at "$days" "$work" push -u origin "$branch"
  git_at "$days" "$work" push origin --delete "$branch"
  git_at "$days" "$work" fetch --prune
}

worktree() {
  local days=$1 work=$ROOT/$2 name=$3
  git_at "$days" "$work" worktree add -b "wt/$name" "$ROOT/.worktrees/$name"
}

stash() {
  local work=$ROOT/$1
  echo "in progress" > "$work/scratch.txt"
  git -C "$work" add scratch.txt >/dev/null
  git -C "$work" -c user.name=Demo -c user.email=demo@example.invalid \
      stash push -m "half-finished refactor" >/dev/null 2>&1 || true
}

# Weight, so the sizes on screen are measurements rather than zeroes. Takes
# whole megabytes; `bs=1048576` rather than `bs=1M` because BSD dd and GNU dd
# disagree about the suffix.
#
# The marker beside it is not decoration. reap refuses to call a directory build
# output unless a sibling file proves what it is — a `target` is only a target
# next to a Cargo.toml — so a fixture without one is a fixture reap will
# correctly ignore. Backdated afterwards, since nothing created a moment ago is
# stale.
bulk() {
  local repo=$1 dir=$2 megabytes=$3 marker=$4
  mkdir -p "$ROOT/$repo/$dir"
  # Incompressible, so the figure survives any filesystem cleverness.
  dd if=/dev/urandom of="$ROOT/$repo/$dir/blob.bin" bs=1048576 count="$megabytes" 2>/dev/null
  [ -f "$ROOT/$repo/$marker" ] || echo '{}' > "$ROOT/$repo/$marker"
  local stamp
  stamp=$(date -v-90d +"%Y%m%d%H%M" 2>/dev/null || date -d "90 days ago" +"%Y%m%d%H%M")
  touch -t "$stamp" "$ROOT/$repo/$dir/blob.bin" "$ROOT/$repo/$dir" "$ROOT/$repo/$marker"
}

for r in api-gateway web-client billing-service search-indexer; do new_repo "$r"; done

merged        210 api-gateway    feature/rate-limiting
merged        150 api-gateway    fix/retry-backoff
squash_merged 120 api-gateway    feature/audit-log
pushed         95 api-gateway    feature/webhook-retries
pushed         60 api-gateway    chore/bump-deps
unpushed      180 api-gateway    spike/cache-layer
unpushed       75 api-gateway    fix/timeout-handling
orphaned      140 api-gateway    feature/batch-endpoint
worktree       88 api-gateway    audit
stash             api-gateway

merged        190 web-client     feature/dark-mode
merged         85 web-client     fix/focus-trap
squash_merged 110 web-client     chore/drop-polyfills
pushed         70 web-client     feature/offline-cache
unpushed      160 web-client     spike/virtual-scroll
worktree       55 web-client     offline

merged        130 billing-service feature/proration
pushed        100 billing-service fix/rounding
unpushed      200 billing-service spike/ledger-rewrite

merged        170 search-indexer  feature/synonyms
pushed        115 search-indexer  chore/reindex-job
pushed         45 search-indexer  feature/fuzzy-match

bulk api-gateway     node_modules  48 package.json
bulk web-client      node_modules 120 package.json
bulk billing-service target        64 Cargo.toml
bulk search-indexer  target        32 Cargo.toml

echo "fixture ready: $ROOT"
