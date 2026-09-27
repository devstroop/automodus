#!/usr/bin/env bash
# Engine-matrix smoke suite: run the standalone example workflows against one
# or more browser engines and fail if any workflow fails.
#
# Usage:
#   scripts/smoke.sh                 # all engines: chromium firefox lightpanda
#   scripts/smoke.sh chromium        # one engine
#   scripts/smoke.sh chromium firefox
#
# Env:
#   AUTOMODUS_BIN   path to the automodus binary (default: ./target/debug/automodus)
#   SMOKE_TIMEOUT   per-workflow timeout in seconds (default: 90)
set -euo pipefail

cd "$(dirname "$0")/.."

BIN="${AUTOMODUS_BIN:-./target/debug/automodus}"
TIMEOUT_SECS="${SMOKE_TIMEOUT:-90}"

if [[ ! -x "$BIN" ]]; then
  echo "Building automodus..."
  cargo build -q
fi

engines=("$@")
if [[ ${#engines[@]} -eq 0 ]]; then
  engines=(chromium firefox lightpanda)
fi

# Workflows that run standalone: no required params, no WhatsApp session,
# no headed-browser requirement. Keep in sync with examples/README.md.
workflows=(
  browser/screenshot.yaml
  browser/scrape_list.yaml
  browser/search_form.yaml
  browser/selectors.yaml
  browser/multi_tab.yaml
  control-flow/branching.yaml
  control-flow/retry_wait.yaml
  compose/pipeline.yaml
  debug/debug_demo.yaml
  http/http_api.yaml
  http/headers_auth.yaml
)

RUNLOG_DIR="$(mktemp -d /tmp/automodus-smoke-runs-XXXXXX)"
CFG=""
trap 'rm -f "${CFG:-}"' EXIT

echo "== validate =="
"$BIN" validate examples

fail=0
for engine in "${engines[@]}"; do
  echo
  echo "== engine: $engine =="
  CFG="$(mktemp /tmp/automodus-smoke-XXXXXX.toml)"
  printf '[browser]\nengine = "%s"\nheadless = true\n' "$engine" >"$CFG"

  for wf in "${workflows[@]}"; do
    # Lightpanda supports a single page per connection upstream
    # (lightpanda-io/browser#882: Target.createTarget fails with
    # TargetAlreadyLoaded), so multi-tab workflows only run on
    # chromium/firefox.
    if [[ "$engine" == "lightpanda" && "$wf" == "browser/multi_tab.yaml" ]]; then
      echo "  SKIP  $wf (lightpanda: single-page upstream limitation)"
      continue
    fi
    runlog="$RUNLOG_DIR/${engine}__$(echo "$wf" | tr '/' '_').log"
    rc=0
    AUTOMODUS_CONFIG="$CFG" timeout "$TIMEOUT_SECS" "$BIN" run "examples/$wf" \
      >"$runlog" 2>&1 || rc=$?
    if [[ $rc -eq 0 ]]; then
      echo "  PASS  $wf"
    else
      echo "  FAIL  $wf (exit $rc, log: $runlog)"
      tail -30 "$runlog" | sed 's/^/    | /'
      fail=1
    fi
    # timeout SIGTERM's automodus, whose destructors never run — reap any
    # browser it spawned. Patterns only match runner-spawned processes;
    # pkill never signals itself, and the smoke script's own argv is safe.
    if [[ $rc -eq 124 || $rc -eq 143 ]]; then
      pkill -f "lightpanda serve --host 127.0.0.1" 2>/dev/null || true
      pkill -f "/tmp/automodus-workflow-" 2>/dev/null || true
    fi
  done

  # A failed lightpanda run must not leave `serve` behind (KillOnDrop guard).
  if [[ "$engine" == "lightpanda" ]] && pgrep -f "lightpanda serve --host 127.0.0.1" >/dev/null 2>&1; then
    echo "  FAIL  orphaned lightpanda serve process after $engine runs"
    pgrep -af "lightpanda serve" | sed 's/^/    | /'
    fail=1
  fi
  rm -f "$CFG"
done

echo
if [[ $fail -ne 0 ]]; then
  echo "SMOKE FAILED (per-run logs: $RUNLOG_DIR)"
  exit 1
fi
rm -rf "$RUNLOG_DIR"
echo "SMOKE OK (${#engines[@]} engine(s), ${#workflows[@]} workflows each)"
