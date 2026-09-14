#!/usr/bin/env bash
# Generates a reproducible, versioned performance target snapshot for the
# WorkTree real-repo benchmarks (benches/performance/real_repo.rs).
#
# The harness RealRepoFixture::from_snapshot_root reads, for each scenario, a
# metadata.json under <snapshot_root>/<case_name>/ whose `source` field is
# resolved relative to that case directory. This script therefore materializes
# a self-contained, relocatable snapshot root:
#
#   <snapshot_root>/
#     source.git/                                   # local clone of the pinned target
#     monorepo_open_and_history_load/metadata.json
#     deep_history_open_and_scroll/metadata.json
#     mid_merge_conflict_list_and_open/metadata.json   # only if --conflict-* given
#     large_file_diff_open/metadata.json               # only if --diff-path given
#     manifest.json                                # the versioned, committable record
#
# Point WORKTREE_PERF_REAL_REPO_ROOT at <snapshot_root> to run the real_repo
# benchmarks. The snapshot lives under tmp/ (git-ignored); only manifest.json is
# meant to be committed so the numbers in README/docs are reproducible.
set -euo pipefail

generator_version="1"
schema_version=1

usage() {
  cat <<'EOF'
Usage: scripts/generate-perf-target-manifest.sh [options]

Materialize a pinned, reproducible real-repo performance target and emit a
versioned manifest (repo, commit sha, clone params, size, ref/commit counts).

Required:
  --repo-url URL     Git URL of the fixed target repository (README names a
                     Chromium-scale monorepo as the reference tier).
  --ref REF          Branch / tag / sha to pin. The resolved commit sha is
                     recorded in the manifest so the snapshot is reproducible.
  --name LABEL       Short label; also the snapshot sub-directory name.

Options:
  --work-dir DIR     Base directory for the snapshot root.
                     Default: <repo_root>/tmp/perf-real-repo
  --manifest-out PATH
                     Where to write the versioned manifest.
                     Default: <repo_root>/benches/performance/real_repo_target.json
  --shallow          Clone with --depth 1. Faster, but commit_count / rev-list
                     metrics will be truncated; not recommended for the canonical
                     target because deep_history scenario expects a long history.
  --conflict-merge-ref REF
                     Ref to merge for mid_merge_conflict_list_and_open. Required
                     to enable that scenario; without it the case dir is skipped.
  --conflict-path PATH
                     Repo-relative path that is conflicted after the merge.
  --diff-path PATH   Repo-relative path for large_file_diff_open. Required to
                     enable that scenario; without it the case dir is skipped.
  --diff-commitish COMMITISH
                     Commitish to diff for large_file_diff_open. Default: HEAD.
  --git-bin PATH     git binary. Default: git
  --force            Overwrite an existing snapshot root.
  -h, --help         Show this help.

Notes:
  - The clone is full by default (single-branch off) so ref/commit counts are
    authoritative. Expect multi-GB targets for Chromium-scale repos.
  - Re-running reuses source.git and only refreshes the pinned ref + metrics.
EOF
}

repo_url=""
ref=""
name=""
work_dir=""
manifest_out=""
shallow=0
conflict_merge_ref=""
conflict_path=""
diff_path=""
diff_commitish="HEAD"
git_bin="git"
force=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo-url) repo_url="$2"; shift 2 ;;
    --ref) ref="$2"; shift 2 ;;
    --name) name="$2"; shift 2 ;;
    --work-dir) work_dir="$2"; shift 2 ;;
    --manifest-out) manifest_out="$2"; shift 2 ;;
    --shallow) shallow=1; shift ;;
    --conflict-merge-ref) conflict_merge_ref="$2"; shift 2 ;;
    --conflict-path) conflict_path="$2"; shift 2 ;;
    --diff-path) diff_path="$2"; shift 2 ;;
    --diff-commitish) diff_commitish="$2"; shift 2 ;;
    --git-bin) git_bin="$2"; shift 2 ;;
    --force) force=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ -z "${repo_url}" || -z "${ref}" || -z "${name}" ]]; then
  echo "Missing required --repo-url / --ref / --name" >&2
  usage >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: "${work_dir:=${repo_root}/tmp/perf-real-repo}"
: "${manifest_out:=${repo_root}/benches/performance/real_repo_target.json}"

snapshot_root="${work_dir}/${name}"
source_repo="${snapshot_root}/source.git"

if [[ ${force} -eq 1 && -d "${snapshot_root}" ]]; then
  rm -rf "${snapshot_root}"
fi

mkdir -p "${snapshot_root}"

g() { "${git_bin}" "$@"; }

echo "==> Cloning target into ${source_repo}"
if [[ ! -d "${source_repo}" ]]; then
  clone_args=(clone --quiet)
  if [[ ${shallow} -eq 1 ]]; then
    clone_args+=(--depth 1)
  fi
  clone_args+=(--no-single-branch "${repo_url}" "${source_repo}")
  g "${clone_args[@]}"
fi

# Ensure the pinned ref is present (covers a sha, a non-default branch, or a tag).
if ! g -C "${source_repo}" rev-parse --verify --quiet "${ref}" >/dev/null 2>&1; then
  echo "==> Fetching pinned ref ${ref}"
  g -C "${source_repo}" fetch --quiet origin "${ref}" || \
    g -C "${source_repo}" fetch --quiet --tags origin "${ref}"
fi

# Resolve the commit sha. A cloned repo only has a local branch for the default
# branch; other branches live under origin/<ref> and tags under refs/tags/<ref>.
commit_sha="$(g -C "${source_repo}" rev-parse --verify "${ref}" 2>/dev/null \
  || g -C "${source_repo}" rev-parse --verify "origin/${ref}" 2>/dev/null \
  || g -C "${source_repo}" rev-parse --verify "refs/tags/${ref}" 2>/dev/null)"
if [[ -z "${commit_sha}" ]]; then
  echo "Could not resolve pinned ref '${ref}' in ${source_repo}" >&2
  exit 1
fi

# Metrics ---------------------------------------------------------------------
commit_count="$(g -C "${source_repo}" rev-list --count "${commit_sha}")"
ref_count="$(g -C "${source_repo}" show-ref | wc -l | tr -d ' ')"
branch_count="$(g -C "${source_repo}" for-each-ref --format='%(refname)' 'refs/heads/*' 'refs/remotes/origin/*' | wc -l | tr -d ' ')"
tag_count="$(g -C "${source_repo}" for-each-ref --format='%(refname)' refs/tags | wc -l | tr -d ' ')"
remote_count="$(g -C "${source_repo}" remote | wc -l | tr -d ' ')"

# .git size in bytes (size-pack + size are KiB in git count-objects -v).
objects_kib="$(g -C "${source_repo}" count-objects -v | awk -F': +' '
  /^size-pack:/ { pack += $2 }
  /^size:/      { loose += $2 }
  END { print pack + loose }')"
source_size_bytes="$(( objects_kib * 1024 ))"
source_size_human="$(du -sh "${source_repo}" 2>/dev/null | cut -f1 || echo "${source_size_bytes} bytes")"

git_version="$(${git_bin} --version | awk '{print $3}')"
generated_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

echo "==> Target: ${repo_url}"
echo "    ref=${ref} commit=${commit_sha}"
echo "    commits=${commit_count} refs=${ref_count} branches=${branch_count} tags=${tag_count} remotes=${remote_count}"
echo "    source size=${source_size_human} (${source_size_bytes} bytes)"

# Scenario metadata -----------------------------------------------------------
write_case_metadata() {
  local case_dir="$1"
  local json="$2"
  mkdir -p "${case_dir}"
  printf '%s\n' "${json}" > "${case_dir}/metadata.json"
}

write_case_metadata "${snapshot_root}/monorepo_open_and_history_load" "$(cat <<JSON
{
  "source": "../source.git",
  "checkout_ref": "${commit_sha}",
  "history_limit": 10000,
  "history_page_size": 1000,
  "history_window": 200
}
JSON
)"

write_case_metadata "${snapshot_root}/deep_history_open_and_scroll" "$(cat <<JSON
{
  "source": "../source.git",
  "checkout_ref": "${commit_sha}",
  "history_limit": 50000,
  "history_page_size": 1000,
  "history_window": 200
}
JSON
)"

conflict_enabled="false"
if [[ -n "${conflict_merge_ref}" && -n "${conflict_path}" ]]; then
  conflict_enabled="true"
  write_case_metadata "${snapshot_root}/mid_merge_conflict_list_and_open" "$(cat <<JSON
{
  "source": "../source.git",
  "merge_ref": "${conflict_merge_ref}",
  "conflict_path": "${conflict_path}"
}
JSON
)"
fi

diff_enabled="false"
if [[ -n "${diff_path}" ]]; then
  diff_enabled="true"
  write_case_metadata "${snapshot_root}/large_file_diff_open" "$(cat <<JSON
{
  "source": "../source.git",
  "diff_path": "${diff_path}",
  "diff_commitish": "${diff_commitish}"
}
JSON
)"
fi

# Versioned manifest ----------------------------------------------------------
regenerate_command="scripts/generate-perf-target-manifest.sh --repo-url ${repo_url} --ref ${ref} --name ${name}"
if [[ ${shallow} -eq 1 ]]; then regenerate_command+=" --shallow"; fi
if [[ -n "${conflict_merge_ref}" ]]; then regenerate_command+=" --conflict-merge-ref ${conflict_merge_ref}"; fi
if [[ -n "${conflict_path}" ]]; then regenerate_command+=" --conflict-path ${conflict_path}"; fi
if [[ -n "${diff_path}" ]]; then regenerate_command+=" --diff-path ${diff_path}"; fi

mkdir -p "$(dirname "${manifest_out}")"
cat > "${manifest_out}" <<JSON
{
  "schema_version": ${schema_version},
  "generator": "scripts/generate-perf-target-manifest.sh",
  "generator_version": "${generator_version}",
  "generated_at": "${generated_at}",
  "git_version": "${git_version}",
  "target": {
    "repo_url": "${repo_url}",
    "ref": "${ref}",
    "commit_sha": "${commit_sha}",
    "shallow": $([[ ${shallow} -eq 1 ]] && echo true || echo false)
  },
  "snapshot_root": "tmp/perf-real-repo/${name}",
  "metrics": {
    "commit_count": ${commit_count},
    "ref_count": ${ref_count},
    "branch_count": ${branch_count},
    "tag_count": ${tag_count},
    "remote_count": ${remote_count},
    "source_size_bytes": ${source_size_bytes},
    "source_size_human": "${source_size_human}"
  },
  "scenarios": [
    { "case": "monorepo_open_and_history_load", "enabled": true,  "checkout_ref": "${commit_sha}", "history_limit": 10000 },
    { "case": "deep_history_open_and_scroll",    "enabled": true,  "checkout_ref": "${commit_sha}", "history_limit": 50000 },
    { "case": "mid_merge_conflict_list_and_open", "enabled": ${conflict_enabled}, "merge_ref": "${conflict_merge_ref}", "conflict_path": "${conflict_path}" },
    { "case": "large_file_diff_open",             "enabled": ${diff_enabled},     "diff_path": "${diff_path}", "diff_commitish": "${diff_commitish}" }
  ],
  "regenerate_command": "${regenerate_command}"
}
JSON

echo
echo "==> Wrote manifest: ${manifest_out}"
echo "==> Snapshot root:  ${snapshot_root}"
echo "    Run real_repo benchmarks with: WORKTREE_PERF_REAL_REPO_ROOT=${snapshot_root} cargo bench -p worktree-ui-gpui --features benchmarks --bench performance real_repo"
