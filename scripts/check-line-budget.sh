#!/bin/sh
set -eu

# Two-tier line budget (mirrors the policy documented in AGENTS.md):
#
#   * Production-only `.rs` files .................... <= 1000 lines
#   * Files carrying test code ...................... <= 1200 lines
#       - any file under a `tests/` directory, OR
#       - a production file with an inline `#[cfg(test)] mod tests { ... }`
#         block (the extra 200 lines is the test budget; it must not be
#         used to grow production logic).
#
# The higher allowance exists ONLY because test code lives alongside the
# code under test; the 1000-line cap on pure production logic is unchanged.

prod_limit=1000
test_limit=1200
offenders=/tmp/vespertide-line-budget-offenders.txt
: > "$offenders"

{ git ls-files '*.rs'; git ls-files --others --exclude-standard '*.rs'; } |
  grep -v '^target/' |
  grep -v '^examples/app/src/models/' |
  while IFS= read -r file; do
    [ -f "$file" ] || continue
    lines=$(wc -l < "$file")
    lines=${lines##*[!0-9]}

    # Decide the budget for this file.
    case "$file" in
      */tests/*)
        limit=$test_limit
        ;;
      *)
        if grep -q '^[[:space:]]*mod tests {' "$file"; then
          limit=$test_limit
        else
          limit=$prod_limit
        fi
        ;;
    esac

    if [ "$lines" -gt "$limit" ]; then
      printf '%s %s (limit %s)\n' "$lines" "$file" "$limit"
    fi
  done > "$offenders"

if [ -s "$offenders" ]; then
  printf 'Rust files exceeding their line budget (production <=%s, with tests <=%s):\n' \
    "$prod_limit" "$test_limit"
  cat "$offenders"
  exit 1
fi

printf 'All tracked Rust files are within budget (production <=%s, with tests <=%s).\n' \
  "$prod_limit" "$test_limit"
