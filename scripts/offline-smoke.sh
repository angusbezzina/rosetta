#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
  printf 'usage: %s /absolute/path/to/rosetta\n' "$0" >&2
  exit 2
fi

rosetta_bin=$1
case "$rosetta_bin" in
  /*) ;;
  *) printf 'rosetta binary path must be absolute\n' >&2; exit 2 ;;
esac

test_dir=$(mktemp -d "${TMPDIR:-/tmp}/rosetta-offline.XXXXXX")
server_pid=
cleanup() {
  if [ -n "$server_pid" ]; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -rf "$test_dir"
}
trap cleanup EXIT HUP INT TERM
cd "$test_dir"
git init -q
mkdir -p src
printf '[package]\nname = "offline-example"\nversion = "0.1.0"\nedition = "2021"\n' > Cargo.toml
printf 'pub fn hello() -> bool { true }\n' > src/lib.rs
git add Cargo.toml src/lib.rs

"$rosetta_bin" init >/dev/null
"$rosetta_bin" glossary check >/dev/null
"$rosetta_bin" atlas check >/dev/null
"$rosetta_bin" glossary context --for src/lib.rs --format json >/dev/null
"$rosetta_bin" atlas context --for src/lib.rs --json >/dev/null
"$rosetta_bin" glossary draft --request "$test_dir/draft" >/dev/null
cat > "$test_dir/draft/answer.json" <<'JSON'
{
  "purpose": "Names used in the offline example.",
  "sections": [{"id": "concepts", "title": "Concepts"}],
  "terms": [{"name": "Workbench", "section": "concepts", "definition": "The local example repository."}]
}
JSON
"$rosetta_bin" glossary draft --from "$test_dir/draft/answer.json" >/dev/null
"$rosetta_bin" glossary check >/dev/null
"$rosetta_bin" atlas bundle --out "$test_dir/bundle.json" >/dev/null
"$rosetta_bin" atlas export >/dev/null
"$rosetta_bin" atlas site --out "$test_dir/site" >/dev/null

test -s "$test_dir/draft/request.json"
test -s "$test_dir/bundle.json"
test -s "$test_dir/ATLAS.md"
test -s "$test_dir/site/index.html"
test -s "$test_dir/site/site-viewer.js"
if [ "${ROSETTA_TEST_LOOPBACK:-0}" = 1 ]; then
  "$rosetta_bin" atlas open --port 0 > "$test_dir/server-url" 2> "$test_dir/server-error" &
  server_pid=$!
  attempts=0
  while [ ! -s "$test_dir/server-url" ] && [ "$attempts" -lt 50 ]; do
    sleep 0.05
    attempts=$((attempts + 1))
  done
  test -s "$test_dir/server-url" || { cat "$test_dir/server-error" >&2; exit 1; }
  server_url=$(head -n 1 "$test_dir/server-url")
  curl --noproxy '*' -fsS "${server_url}session.json" | grep -q '"local":true'
  curl --noproxy '*' -fsS "${server_url}bundle.json" | grep -q '"schema"'
fi
printf 'offline local workflow passed\n'
