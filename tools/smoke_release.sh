#!/usr/bin/env bash
# Smoke-test a release binary on its own, away from the source tree: it
# starts, serves the web UI built into it and the API, and converts a model.
#
#     tools/smoke_release.sh <schemgen2 binary> <model.glb>
#
# The release workflow runs it on each platform it builds natively.
set -euo pipefail

binary="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
model="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
work="$(mktemp -d)"
server=""
cleanup() {
  if [ -n "$server" ]; then
    kill "$server" 2>/dev/null || true
    wait "$server" 2>/dev/null || true
  fi
  cd /
  # Best effort: a file the server just let go of may still be locked on Windows.
  rm -rf "$work" || true
}
trap cleanup EXIT
cd "$work"

"$binary" --version

"$binary" serve --port 0 --work-dir "$work/server" --job-ttl 0 >out.txt 2>err.txt </dev/null &
server=$!

url=""
for _ in $(seq 1 100); do
  url="$(sed -n 's/^listening //p' out.txt | tr -d '\r')"
  [ -n "$url" ] && break
  sleep 0.1
done
if [ -z "$url" ]; then
  echo "The server did not start:" >&2
  cat err.txt >&2
  exit 1
fi
if ! grep -q "built into this binary" err.txt; then
  echo "The web UI is not built into the binary:" >&2
  cat err.txt >&2
  exit 1
fi

curl -fsS "$url/" -o index.html
grep -q '<div id="root">' index.html
script="$(grep -o 'assets/[^"]*\.js' index.html | head -n 1)"
curl -fsS "$url/$script" -o /dev/null
curl -fsS "$url/api/health" | grep -q '"status":"ok"'

"$binary" convert "$model" --max-size 32 -o "$work/smoke.litematic" 2>convert.txt
test -s "$work/smoke.litematic"
echo "Smoke test passed: $("$binary" --version) at $url"
