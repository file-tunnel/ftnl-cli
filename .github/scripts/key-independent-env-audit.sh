#!/usr/bin/env bash
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

fail=0
note() {
  printf '  ✗ %s\n' "$1" >&2
  fail=1
}

printf 'tracked files:\n'
bad="$({ git ls-files -z 2>/dev/null || true; } | tr '\0' '\n' \
  | grep -E '(^|/)\.env$|(^|/)\.env\.[^/]*$|(^|/)env/dec/' \
  | grep -vE '\.(example|sample|template)$' || true)"
if [[ -n "$bad" ]]; then
  note "plaintext env files are tracked"
  printf '%s\n' "$bad" | sed 's/^/      /'
else
  printf '  ✓ no dotenv secrets tracked\n'
fi

forced="$({ git ls-files -z 2>/dev/null || true; } | tr '\0' '\n' \
  | git check-ignore --no-index --stdin 2>/dev/null \
  | grep -E '(^|/)env/|\.env$|\.age-?key$|(^|/)keys\.txt$|sops-private' || true)"
if [[ -n "$forced" ]]; then
  note "tracked files bypass .gitignore"
  printf '%s\n' "$forced" | sed 's/^/      /'
else
  printf '  ✓ nothing force-added past .gitignore\n'
fi

printf 'private key material:\n'
keys="$({ git ls-files -z 2>/dev/null || true; } | tr '\0' '\n' \
  | grep -E '\.(agekey|age-key)$|(^|/)keys\.txt$|AGE-SECRET-KEY' || true)"
if [[ -n "$keys" ]]; then
  note "possible age private keys are tracked"
  printf '%s\n' "$keys" | sed 's/^/      /'
else
  printf '  ✓ no age private keys tracked\n'
fi

printf '.gitignore:\n'
for rule in '.env' '*.env' '**/*.env' 'env/dec/'; do
  if grep -qxF "$rule" .gitignore 2>/dev/null; then
    printf '  ✓ %s\n' "$rule"
  else
    note "missing .gitignore rule: $rule"
  fi
done

printf 'ciphertext integrity:\n'
shopt -s nullglob
ciphertexts=(env/enc/*.env.enc)
if [[ ${#ciphertexts[@]} -eq 0 ]]; then
  printf '  · no encrypted environments yet\n'
fi
for file in "${ciphertexts[@]}"; do
  if ! grep -q 'ENC\[AES256_GCM' "$file"; then
    note "$file is not SOPS-encrypted"
    continue
  fi
  grep -q '^sops_mac=' "$file" || note "$file has no SOPS MAC"
  recipient_count="$(grep -c 'map_recipient' "$file" || true)"
  if [[ "$recipient_count" -lt 2 ]]; then
    note "$file has $recipient_count recipient(s); at least two are required"
  else
    printf '  ✓ %s encrypted with %s recipients\n' "$file" "$recipient_count"
  fi
done

if [[ "$fail" -ne 0 ]]; then
  printf '\nkey-independent env audit FAILED\n' >&2
  exit 1
fi
printf '\nkey-independent env audit PASSED\n'
