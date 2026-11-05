#!/usr/bin/env bash
set -euo pipefail

matches=0
while IFS= read -r -d '' file; do
  if [[ ! -f "$file" ]]; then
    continue
  fi
  if rg --pcre2 -q '(-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9_]{20,})' "$file"; then
    echo "Potential credential marker: $file" >&2
    matches=1
  fi
done < <(git ls-files -z)

exit "$matches"
