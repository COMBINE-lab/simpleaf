#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: release.sh <version> [--push] [--dry-run]

Creates a release by:
1. Validating <version> as semantic versioning (SemVer 2.0.0)
2. Ensuring <version> is greater than the current Cargo.toml package version
3. Ensuring tag v<version> does not already exist
4. Updating Cargo.toml [package].version
5. Committing Cargo.toml and creating tag v<version>
6. Optionally pushing the tag when --push is provided
7. With --dry-run, print planned actions without changing git state/files
USAGE
}

err() {
  echo "Error: $*" >&2
  exit 1
}

is_valid_semver() {
  local version="$1"
  local semver_regex='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*))?(\+([0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*))?$'
  printf '%s\n' "$version" | grep -Eq "$semver_regex"
}

split_semver() {
  local version="$1"
  local core="$version"
  local pre=""
  local major minor patch

  if [[ "$core" == *+* ]]; then
    core="${core%%+*}"
  fi

  if [[ "$core" == *-* ]]; then
    pre="${core#*-}"
    core="${core%%-*}"
  fi

  IFS='.' read -r major minor patch <<<"$core"
  printf '%s\t%s\t%s\t%s\n' "$major" "$minor" "$patch" "$pre"
}

compare_prerelease() {
  local left="$1"
  local right="$2"

  if [[ -z "$left" && -z "$right" ]]; then
    echo 0
    return
  fi
  if [[ -z "$left" ]]; then
    echo 1
    return
  fi
  if [[ -z "$right" ]]; then
    echo -1
    return
  fi

  local -a lparts=()
  local -a rparts=()
  IFS='.' read -r -a lparts <<<"$left"
  IFS='.' read -r -a rparts <<<"$right"

  local i
  local lnum rnum
  for ((i = 0; i < ${#lparts[@]} || i < ${#rparts[@]}; i++)); do
    if ((i >= ${#lparts[@]})); then
      echo -1
      return
    fi
    if ((i >= ${#rparts[@]})); then
      echo 1
      return
    fi

    if [[ "${lparts[$i]}" == "${rparts[$i]}" ]]; then
      continue
    fi

    if [[ "${lparts[$i]}" =~ ^(0|[1-9][0-9]*)$ ]]; then
      lnum=1
    else
      lnum=0
    fi
    if [[ "${rparts[$i]}" =~ ^(0|[1-9][0-9]*)$ ]]; then
      rnum=1
    else
      rnum=0
    fi

    if ((lnum == 1 && rnum == 1)); then
      if ((10#${lparts[$i]} > 10#${rparts[$i]})); then
        echo 1
      else
        echo -1
      fi
      return
    fi
    if ((lnum == 1 && rnum == 0)); then
      echo -1
      return
    fi
    if ((lnum == 0 && rnum == 1)); then
      echo 1
      return
    fi

    if [[ "${lparts[$i]}" > "${rparts[$i]}" ]]; then
      echo 1
    else
      echo -1
    fi
    return
  done

  echo 0
}

compare_semver() {
  local left="$1"
  local right="$2"

  local lmaj lmin lpat lpre
  local rmaj rmin rpat rpre
  IFS=$'\t' read -r lmaj lmin lpat lpre <<<"$(split_semver "$left")"
  IFS=$'\t' read -r rmaj rmin rpat rpre <<<"$(split_semver "$right")"

  if ((10#$lmaj > 10#$rmaj)); then
    echo 1
    return
  fi
  if ((10#$lmaj < 10#$rmaj)); then
    echo -1
    return
  fi

  if ((10#$lmin > 10#$rmin)); then
    echo 1
    return
  fi
  if ((10#$lmin < 10#$rmin)); then
    echo -1
    return
  fi

  if ((10#$lpat > 10#$rpat)); then
    echo 1
    return
  fi
  if ((10#$lpat < 10#$rpat)); then
    echo -1
    return
  fi

  compare_prerelease "$lpre" "$rpre"
}

push_tag=false
dry_run=false
version=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --push)
      push_tag=true
      shift
      ;;
    --dry-run)
      dry_run=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      err "Unknown option: $1"
      ;;
    *)
      if [[ -n "$version" ]]; then
        err "Provide exactly one version argument."
      fi
      version="$1"
      shift
      ;;
  esac
done

[[ -n "$version" ]] || { usage >&2; exit 1; }

[[ -f "Cargo.toml" ]] || err "Cargo.toml not found in current directory."
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || err "Current directory is not a git repository."

is_valid_semver "$version" || err "Version '$version' is not valid SemVer 2.0.0."

current_version="$(
  awk '
    /^\[package\]/ { in_package=1; next }
    in_package && /^\[/ { in_package=0 }
    in_package && /^[[:space:]]*version[[:space:]]*=/ {
      match($0, /"[^"]+"/)
      if (RSTART > 0) {
        print substr($0, RSTART + 1, RLENGTH - 2)
        exit
      }
    }
  ' Cargo.toml
)"

[[ -n "$current_version" ]] || err "Could not find [package].version in Cargo.toml."
is_valid_semver "$current_version" || err "Current Cargo.toml version '$current_version' is not valid SemVer."

cmp="$(compare_semver "$version" "$current_version")"
if [[ "$cmp" != "1" ]]; then
  err "Version '$version' must be greater than current version '$current_version'."
fi

tag="v${version}"
if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null 2>&1; then
  err "Tag '${tag}' already exists locally."
fi
if git remote get-url origin >/dev/null 2>&1; then
  if git ls-remote --exit-code --tags origin "refs/tags/${tag}" >/dev/null 2>&1; then
    err "Tag '${tag}' already exists on remote 'origin'."
  fi
fi

tmp_file="$(mktemp)"
cleanup() {
  rm -f "$tmp_file"
}
trap cleanup EXIT

awk -v new_version="$version" '
  BEGIN { in_package=0; updated=0 }
  /^\[package\]/ { in_package=1; print; next }
  in_package && /^\[/ { in_package=0 }
  in_package && !updated && /^[[:space:]]*version[[:space:]]*=/ {
    print "version = \"" new_version "\""
    updated=1
    next
  }
  { print }
  END {
    if (!updated) {
      exit 2
    }
  }
' Cargo.toml > "$tmp_file" || err "Failed to update Cargo.toml."

if [[ "$dry_run" == true ]]; then
  echo "Dry run successful:"
  echo "- Current version: ${current_version}"
  echo "- New version: ${version}"
  echo "- Tag to create: ${tag}"
  echo "- Would update Cargo.toml [package].version to ${version}"
  echo "- Would commit: Release ${tag}"
  echo "- Would create tag: ${tag}"
  if [[ "$push_tag" == true ]]; then
    git remote get-url origin >/dev/null 2>&1 || err "No 'origin' remote configured, cannot push tag."
    echo "- Would push tag to origin: ${tag}"
  fi
  exit 0
fi

mv "$tmp_file" Cargo.toml

git add Cargo.toml
git commit -m "Release ${tag}" -- Cargo.toml
git tag "${tag}"

if [[ "$push_tag" == true ]]; then
  git remote get-url origin >/dev/null 2>&1 || err "No 'origin' remote configured, cannot push tag."
  git push origin "${tag}"
fi

echo "Released ${tag} (updated Cargo.toml from ${current_version} to ${version})."
