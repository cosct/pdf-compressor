#!/usr/bin/env bash
# Generate GitHub release notes for a version tag from its CHANGELOG.md
# section — the single-source-of-truth document. The extracted body keeps
# the section's `###` subsection headings promoted to `##` (matching the
# hand-written v0.5.0 notes style) and appends the standard footer links.
#
# Usage: scripts/release-notes.sh <tag> [<changelog-path>] [<repo-slug>]
#   tag             e.g. v0.6.0 (the `## [0.6.0]` section is extracted)
#   changelog-path  defaults to CHANGELOG.md next to the script's repo root
#   repo-slug       defaults to $GH_REPO or cosct/pdf-compressor
# Prints the notes to stdout; fails loudly when the section is missing so a
# release can never ship with an empty body silently.
set -euo pipefail

tag="${1:?usage: release-notes.sh <tag> [changelog] [repo]}"
changelog="${2:-$(dirname "$0")/../CHANGELOG.md}"
repo="${3:-${GH_REPO:-cosct/pdf-compressor}}"

version="${tag#v}"

body=$(awk -v version="$version" '
    # Skip everything before the requested section header.
    $0 == "## [" version "]" || index($0, "## [" version "] ") == 1 { inside = 1; next }
    inside && /^## \[/ { exit }
    inside {
        # Promote subsection headings one level: the CHANGELOG top-level
        # section header is dropped (the release title carries it), so its
        # `###` children become the notes `##` sections.
        if (index($0, "### ") == 1) {
            print "##" substr($0, 4)
        } else {
            print
        }
    }
' "$changelog")

if [ -z "$body" ]; then
    echo "error: CHANGELOG has no section for [$version] — write it before tagging" >&2
    exit 1
fi

# Previous version = the next `## [` header after this section (newest first).
previous=$(awk -v version="$version" '
    $0 == "## [" version "]" || index($0, "## [" version "] ") == 1 { inside = 1; next }
    inside && match($0, /^## \[[^\]]+\]/) {
        section = $0
        sub(/^## \[/, "", section)
        sub(/\].*$/, "", section)
        print section
        exit
    }
' "$changelog")

echo "$body"
echo
echo "**完整变更**: https://github.com/${repo}/blob/main/CHANGELOG.md"
if [ -n "$previous" ]; then
    echo
    echo "**Full Changelog**: https://github.com/${repo}/compare/v${previous}...v${version}"
fi
