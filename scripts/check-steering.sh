#!/usr/bin/env bash
# Enforce CLAUDE.md: no ledgers, a fixed file list, no history in steering
# files, and length caps. A ledger is prose, so the name check reads prose
# files and the folders that hold them. A code module is never a ledger, and
# a source tree is never scanned. No argument checks the root folder. A repo directory
# checks that repo. Exit 1 on any finding.
set -u
root="$(cd "$(dirname "$0")/.." && pwd)"
fail=0
finding() { printf 'check: %s\n' "$1" >&2; fail=1; }

forbidden_names='open-items|open_items|build-history|build_history|decision-log|decision_log|handoff|journal|journal-ideas|notes|todo|roadmap-history|roadmap-reviews|roadmap_history'
prune='-name .git -o -name node_modules -o -name target -o -name dist -o -name build -o -name .venv -o -name .next -o -name .expo -o -name ios -o -name android -o -name src'

check_forbidden() {
  local dir="$1"
  find "$dir" \( $prune \) -prune -o -type f -print 2>/dev/null \
    | while IFS= read -r path; do
        base="$(basename "$path")"; stem="${base%.*}"; ext="${base##*.}"
        case "$ext" in md|markdown|txt|rst|adoc|html|htm|org) ;; *) continue;; esac
        if printf '%s' "$stem" | grep -Eiq "^(${forbidden_names})$"; then
          printf '%s\n' "$path"
        fi
        parent="$(dirname "$path")"
        if printf '%s' "$(basename "$parent")" | grep -Eiq "^(${forbidden_names})$"; then
          printf '%s\n' "$parent"
        fi
      done \
    | sort -u \
    | while IFS= read -r hit; do finding "ledger file or folder: ${hit#"$root"/}"; done
}

check_history() {
  local file="$1"
  [ -f "$file" ] || return 0
  local rel="${file#"$root"/}"
  grep -nE '\b20[0-9]{2}-[0-9]{2}-[0-9]{2}\b' "$file" | head -3 | while IFS= read -r line; do finding "date in steering file $rel: ${line:0:90}"; done
  grep -nE '\b(MUNISITE|MUNICLOUD|MUNIDESK|MUNIMOBILE|MUNIQA|MUNISOCIAL|FACTORY)-[0-9]+\b' "$file" | head -3 | while IFS= read -r line; do finding "ticket id in steering file $rel: ${line:0:90}"; done
  grep -nE '\b[0-9a-f]{7,40}\b' "$file" | grep -vE 'sha256|sha-256|[0-9a-f]{64}' | grep -E '\b[0-9a-f]{7,12}\b' | grep -E '(commit|sha|@)' | head -3 | while IFS= read -r line; do finding "commit sha in steering file $rel: ${line:0:90}"; done
  grep -niE 'owner (ruling|decision)|\bdecided\b|\bwithdrawn\b|\bsuperseded\b|\bpreviously\b|\bno longer\b' "$file" | grep -vi 'history phrase' | head -3 | while IFS= read -r line; do finding "history phrase in steering file $rel: ${line:0:90}"; done
}

check_cap() {
  local file="$1" cap="$2"
  [ -f "$file" ] || return 0
  local n; n="$(wc -l < "$file" | tr -d ' ')"
  [ "$n" -le "$cap" ] || finding "${file#"$root"/} has $n lines, cap $cap"
}

if [ "$#" -eq 0 ]; then
  allow='PLAN.md README.md CLAUDE.md spec brand tools .DS_Store'
  for entry in "$root"/* "$root"/.[!.]*; do
    [ -e "$entry" ] || continue
    name="$(basename "$entry")"
    case " $allow " in *" $name "*) continue;; esac
    [ -d "$entry/.git" ] && continue
    finding "root entry not in the file list: $name"
  done
  for d in spec brand tools; do check_forbidden "$root/$d"; done
  for f in PLAN.md README.md CLAUDE.md; do check_history "$root/$f"; done
  check_cap "$root/PLAN.md" 800
else
  repo="$(cd "$1" && pwd)" || { finding "no such directory: $1"; exit 1; }
  root="$repo"
  check_forbidden "$repo"
  for f in AGENTS.md README.md SPEC.md ROADMAP.md DESIGN.md; do check_history "$repo/$f"; done
  check_cap "$repo/SPEC.md" 600
  check_cap "$repo/DESIGN.md" 250
  check_cap "$repo/ROADMAP.md" 150
  check_cap "$repo/AGENTS.md" 120
fi

if [ "$fail" -eq 0 ]; then echo "check: clean"; fi
exit "$fail"
