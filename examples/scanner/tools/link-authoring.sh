#!/bin/sh
# Link the authoring packages from the installed plugin checkout. The example names the
# plugin (`plugin.name`), so the sidecars import the same `@veridoc/claims` the installed
# plugin was built with — not a copy vendored into this repository.
#
#   VERIDOC_HOME=/path/to/registry tools/link-authoring.sh
set -eu
registry="${VERIDOC_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/veridoc}/plugins"
checkout="$registry/veridoc-plugin-ts/checkout"
if [ ! -d "$checkout" ]; then
    echo "no veridoc-plugin-ts checkout under $registry." >&2
    echo "Install it first: veridoc install-plugin <git-url>" >&2
    exit 1
fi
cd "$(dirname "$0")/.."
npm install --no-save --no-audit --no-fund \
    "file:$checkout/packages/claims" \
    "file:$checkout/packages/tree" \
    "@types/node"
echo "linked @veridoc/claims and @veridoc/tree from $checkout"
