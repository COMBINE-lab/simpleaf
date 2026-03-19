#!/usr/bin/env bash
set -euo pipefail

die() {
    echo "error: $*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage:
  ./bump_and_publish.sh <version> [--publish] [--dry-run]
  ./bump_and_publish.sh [--publish] [--dry-run] <version>

Options:
  --publish  Publish to crates.io after bumping, committing, tagging, and pushing
  --dry-run  Show what would be done without modifying tracked files, creating commits or tags, pushing, or publishing
  -h, --help Show this help message
EOF
}

print_cmd() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
}

run() {
    print_cmd "$@"
    if [[ "$DRY_RUN" == true ]]; then
        return 0
    fi
    "$@"
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

VERSION=""
PUBLISH=false
DRY_RUN=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --publish)
            PUBLISH=true
            ;;
        --dry-run)
            DRY_RUN=true
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -*)
            die "unknown option: $1"
            ;;
        *)
            if [[ -n "$VERSION" ]]; then
                die "version specified more than once"
            fi
            VERSION="$1"
            ;;
    esac
    shift
done

[[ -n "$VERSION" ]] || {
    usage
    exit 1
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

ROOT_CARGO="Cargo.toml"
LOCKFILE="Cargo.lock"
CRATE_NAME="simpleaf"
TAG="v${VERSION}"
TMP_TARGET_DIR=""
MANIFEST_BACKUP=""
LOCKFILE_BACKUP=""
MANIFEST_UPDATED=false
COMMIT_CREATED=false

cleanup() {
    local status=$?

    if [[ -n "$TMP_TARGET_DIR" && -d "$TMP_TARGET_DIR" ]]; then
        rm -rf "$TMP_TARGET_DIR"
    fi

    if [[ "$status" -ne 0 && "$DRY_RUN" == false && "$MANIFEST_UPDATED" == true && "$COMMIT_CREATED" == false ]]; then
        if [[ -n "$MANIFEST_BACKUP" && -f "$MANIFEST_BACKUP" ]]; then
            cp "$MANIFEST_BACKUP" "$ROOT_CARGO"
        fi
        if [[ -n "$LOCKFILE_BACKUP" && -f "$LOCKFILE_BACKUP" ]]; then
            cp "$LOCKFILE_BACKUP" "$LOCKFILE"
        fi
        echo "restored $ROOT_CARGO and $LOCKFILE after failure" >&2
    fi

    if [[ -n "$MANIFEST_BACKUP" && -f "$MANIFEST_BACKUP" ]]; then
        rm -f "$MANIFEST_BACKUP"
    fi
    if [[ -n "$LOCKFILE_BACKUP" && -f "$LOCKFILE_BACKUP" ]]; then
        rm -f "$LOCKFILE_BACKUP"
    fi

    return "$status"
}

trap cleanup EXIT

[[ -f "$ROOT_CARGO" ]] || die "not found: $ROOT_CARGO"
[[ -f "$LOCKFILE" ]] || die "not found: $LOCKFILE"
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "current directory is not a git repository"

is_valid_semver "$VERSION" || die "version '$VERSION' is not valid SemVer 2.0.0"

CURRENT_VERSION="$(
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
    ' "$ROOT_CARGO"
)"
[[ -n "$CURRENT_VERSION" ]] || die "could not determine current crate version from $ROOT_CARGO"
is_valid_semver "$CURRENT_VERSION" || die "current crate version '$CURRENT_VERSION' is not valid SemVer"

CMP="$(compare_semver "$VERSION" "$CURRENT_VERSION")"
if [[ "$CMP" -le 0 ]]; then
    die "new version '$VERSION' must be greater than current version '$CURRENT_VERSION'"
fi

if git rev-parse "$TAG" >/dev/null 2>&1; then
    die "tag $TAG already exists"
fi

if [[ -n "$(git status --porcelain)" ]]; then
    die "working tree is not clean; commit or stash existing changes first"
fi

if ! git remote get-url origin >/dev/null 2>&1; then
    die "git remote 'origin' is not configured"
fi

echo "Current crate version : $CURRENT_VERSION"
echo "New crate version     : $VERSION"
echo "Tag                   : $TAG"
if [[ "$PUBLISH" == true ]]; then
    echo "Publish               : yes"
else
    echo "Publish               : no"
fi
if [[ "$DRY_RUN" == true ]]; then
    echo "Dry-run               : yes"
else
    echo "Dry-run               : no"
fi
echo

echo "Preflight checks before changing version"
cargo check -q
TMP_TARGET_DIR="$(mktemp -d "${TMPDIR:-/tmp}/simpleaf-release-check.XXXXXX")"
CARGO_TARGET_DIR="$TMP_TARGET_DIR" cargo package --offline --allow-dirty --no-verify >/dev/null
rm -rf "$TMP_TARGET_DIR"
TMP_TARGET_DIR=""

echo "Updating $ROOT_CARGO"
echo "  version: $CURRENT_VERSION -> $VERSION"
echo "Updating $LOCKFILE"
echo "  package entry version: $CURRENT_VERSION -> $VERSION"

if [[ "$DRY_RUN" == false ]]; then
    MANIFEST_BACKUP="$(mktemp "${TMPDIR:-/tmp}/simpleaf-Cargo.toml.XXXXXX")"
    LOCKFILE_BACKUP="$(mktemp "${TMPDIR:-/tmp}/simpleaf-Cargo.lock.XXXXXX")"
    cp "$ROOT_CARGO" "$MANIFEST_BACKUP"
    cp "$LOCKFILE" "$LOCKFILE_BACKUP"

    sed -i.bak "1,/^version = /s/^version = \".*\"/version = \"${VERSION}\"/" "$ROOT_CARGO"
    rm -f "${ROOT_CARGO}.bak"

    sed -i.bak "/^name = \"${CRATE_NAME}\"$/,/^dependencies = \\[$/s/^version = \".*\"/version = \"${VERSION}\"/" "$LOCKFILE"
    rm -f "${LOCKFILE}.bak"

    MANIFEST_UPDATED=true
else
    echo "Dry-run: would rewrite $ROOT_CARGO and $LOCKFILE"
fi

UPDATED_VERSION="$CURRENT_VERSION"
UPDATED_LOCK_VERSION="$CURRENT_VERSION"
if [[ "$DRY_RUN" == false ]]; then
    UPDATED_VERSION="$(
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
        ' "$ROOT_CARGO"
    )"
    UPDATED_LOCK_VERSION="$(sed -n "/^name = \"${CRATE_NAME}\"$/,/^dependencies = \\[$/s/^version = \"\(.*\)\"/\1/p" "$LOCKFILE" | head -1)"

    [[ "$UPDATED_VERSION" == "$VERSION" ]] || die "crate version update failed"
    [[ "$UPDATED_LOCK_VERSION" == "$VERSION" ]] || die "lockfile version update failed"
fi

echo
echo "Post-bump validation"
if [[ "$DRY_RUN" == true ]]; then
    echo "Dry-run: would run cargo check and cargo package against the bumped version"
else
    cargo check -q
    TMP_TARGET_DIR="$(mktemp -d "${TMPDIR:-/tmp}/simpleaf-release-check.XXXXXX")"
    CARGO_TARGET_DIR="$TMP_TARGET_DIR" cargo package --offline --allow-dirty --no-verify >/dev/null
    rm -rf "$TMP_TARGET_DIR"
    TMP_TARGET_DIR=""
fi

run git add "$ROOT_CARGO" "$LOCKFILE"
run git commit -m "chore(release): bump ${CRATE_NAME} to v${VERSION}"

if [[ "$DRY_RUN" == false ]]; then
    COMMIT_CREATED=true
fi

run git tag -a "$TAG" -m "Release ${VERSION}"
run git push origin HEAD
run git push origin "$TAG"

if [[ "$PUBLISH" == true ]]; then
    run cargo publish
else
    echo "Skipping crates.io publish; re-run with --publish to publish ${CRATE_NAME} v${VERSION}"
fi

echo
if [[ "$DRY_RUN" == true ]]; then
    echo "Dry-run complete"
else
    echo "Release bump and publish complete for v${VERSION}"
fi
