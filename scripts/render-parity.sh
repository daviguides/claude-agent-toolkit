#!/bin/sh
# Renders docs/sync/parity.yaml into docs/sync/PARITY.md, a markdown
# table grouped by category. Do not hand-edit PARITY.md; edit
# parity.yaml and re-run this script instead.
#
# Requires: yq (https://github.com/mikefarah/yq), jq.
set -eu

cd "$(dirname "$0")/.."

YAML=docs/sync/parity.yaml
OUT=docs/sync/PARITY.md

if ! command -v yq >/dev/null 2>&1; then
  echo "error: yq not found (https://github.com/mikefarah/yq)" >&2
  exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq not found" >&2
  exit 1
fi

{
  echo "<!-- GENERATED from parity.yaml — do not edit -->"
  echo "# Upstream <-> Rust Parity"
  echo
  echo "Machine-readable source: [\`parity.yaml\`](parity.yaml). Regenerate"
  echo "this file with \`scripts/render-parity.sh\` after editing it."
  echo

  upstream_pin=$(yq -o=json '.' "$YAML" | jq -r '.upstream_pin')
  echo "Upstream pin: \`$upstream_pin\`"
  echo

  total=$(yq -o=json '.' "$YAML" | jq '.entries | length')
  ported=$(yq -o=json '.' "$YAML" | jq '[.entries[] | select(.status == "ported")] | length')
  justified=$(yq -o=json '.' "$YAML" | jq '[.entries[] | select(.status == "justified_gap")] | length')
  not_ported=$(yq -o=json '.' "$YAML" | jq '[.entries[] | select(.status == "not_ported")] | length')
  partial=$(yq -o=json '.' "$YAML" | jq '[.entries[] | select(.status == "partial")] | length')

  echo "**Total: $total** — ported: $ported, justified_gap: $justified, not_ported: $not_ported, partial: $partial"
  echo

  categories=$(yq -o=json '.' "$YAML" | jq -r '[.entries[].category] | unique | .[]')
  for category in $categories; do
    echo "## $category"
    echo
    echo "| Upstream symbol | Kind | Rust equivalent | Status | Tested | Notes / Justification |"
    echo "|---|---|---|---|---|---|"
    yq -o=json '.' "$YAML" | jq -r --arg cat "$category" '
      .entries[]
      | select(.category == $cat)
      | [
          .upstream_symbol,
          .upstream_kind,
          (.rust_equivalent // "—"),
          .status,
          (.tested | tostring),
          ((.notes // "") + (if .justification then (if (.notes // "") != "" then " " else "" end) + .justification else "" end))
        ]
      | @tsv
    ' | while IFS=$(printf '\t') read -r symbol kind rust status tested notes; do
      # Escape pipe characters so cell content doesn't break the table.
      symbol=$(printf '%s' "$symbol" | sed 's/|/\\|/g')
      kind=$(printf '%s' "$kind" | sed 's/|/\\|/g')
      rust=$(printf '%s' "$rust" | sed 's/|/\\|/g')
      notes=$(printf '%s' "$notes" | sed 's/|/\\|/g')
      echo "| \`$symbol\` | $kind | \`$rust\` | $status | $tested | $notes |"
    done
    echo
  done
} > "$OUT"

echo "Wrote $OUT"
