#!/usr/bin/env bash
# Publish the workspace's crates to crates.io, each after the crates it
# depends on (dev-dependencies included, since a published manifest keeps
# them), skipping any version the index already has. A run that stops part
# way is finished by running it again.
#
# CARGO_REGISTRY_TOKEN is either a stored crates.io token or the short-lived
# one crates.io trusted publishing hands the release workflow.
set -euo pipefail

: "${CARGO_REGISTRY_TOKEN:?no crates.io token: set the CARGO_REGISTRY_TOKEN secret or turn on trusted publishing}"

# Workspace crates that may be published, dependencies first.
order="$(cargo metadata --format-version 1 --no-deps | python3 -c '
import json, sys
meta = json.load(sys.stdin)
crates = {p["name"]: p for p in meta["packages"] if p.get("publish") != []}
done, out = set(), []
def visit(name, path=()):
    if name in done:
        return
    if name in path:
        sys.exit("dependency cycle: " + " -> ".join(path + (name,)))
    for dep in crates[name]["dependencies"]:
        if dep["name"] in crates and dep.get("path"):
            visit(dep["name"], path + (name,))
    done.add(name)
    out.append(name)
for name in sorted(crates):
    visit(name)
print("\n".join(out))
')"

# The sparse index path of a crate name of four or more characters.
index_url() {
  local name="${1,,}"
  echo "https://index.crates.io/${name:0:2}/${name:2:2}/${name}"
}

for crate in ${order}; do
  version="$(cargo metadata --format-version 1 --no-deps \
    | jq -r --arg c "${crate}" '.packages[] | select(.name == $c) | .version')"
  if curl -sf "$(index_url "${crate}")" | jq -e --arg v "${version}" 'select(.vers == $v)' >/dev/null; then
    echo "${crate} ${version} is already on crates.io"
    continue
  fi
  echo "publishing ${crate} ${version}"
  # cargo waits until the index serves it before the next crate needs it.
  # crates.io limits how many new crates an account publishes in a while and
  # answers 429 with the time to try again; wait until then.
  for attempt in $(seq 1 12); do
    if cargo publish --locked -p "${crate}" 2>&1 | tee "${RUNNER_TEMP:-/tmp}/publish.log"; then
      break
    fi
    when="$(grep -o 'try again after [A-Z][a-z][a-z], [0-9]* [A-Z][a-z]* [0-9]* [0-9:]* GMT' "${RUNNER_TEMP:-/tmp}/publish.log" | head -1 | sed 's/^try again after //')"
    [ -n "${when}" ] && [ "${attempt}" -lt 12 ] || exit 1
    wait=$(( $(date -d "${when}" +%s) - $(date +%s) + 15 ))
    echo "crates.io asks to wait until ${when}; waiting ${wait}s"
    [ "${wait}" -gt 0 ] && sleep "${wait}"
  done
done
