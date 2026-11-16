#!/bin/sh
# Install the veridoc command: bootstrap Rust when none is installed, build a release binary
# from the public repository, and put it somewhere on PATH. No sudo, ever.
#
# Environment overrides:
#   VERIDOC_SRC_URL      repository to build           (default https://github.com/DustinEwan/veridoc.git)
#   VERIDOC_REF          branch or tag to build        (default the repository's default branch)
#   VERIDOC_INSTALL_DIR  where the binaries go         (default $HOME/.local/bin)
#   VERIDOC_WORK_DIR     parent of the build directory (default $TMPDIR or /tmp)
set -eu

SRC_URL=${VERIDOC_SRC_URL:-https://github.com/DustinEwan/veridoc.git}
REF=${VERIDOC_REF:-}
# ${HOME:+} leaves INSTALL_DIR empty when HOME is unset, so the check below stops the run. A plain
# ${HOME:-} would default to /.local/bin, sail past that check, and fail later inside mkdir -p.
INSTALL_DIR=${VERIDOC_INSTALL_DIR:-${HOME:+$HOME/.local/bin}}

# Every message goes to stderr, so a reader who redirects the script's stdout keeps it clean.
say() { printf 'veridoc-install: %s\n' "$1" >&2; }
die() { printf 'veridoc-install: %s\n' "$1" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

[ -n "$INSTALL_DIR" ] || die "HOME is not set; point VERIDOC_INSTALL_DIR at a directory on your PATH"
have curl || die "curl is required; install it first (apt-get install -y curl, brew install curl)"
have git || die "git is required to clone $SRC_URL; install it first (apt-get install -y git, brew install git)"
if ! have cc && ! have gcc && ! have clang; then
  die "a C compiler is required to link the engine; install one first (sudo apt-get install -y build-essential, xcode-select --install)"
fi

# An existing cargo is used as it stands. A toolchain older than the workspace rust-version of
# 1.88 makes cargo itself say so, and the build failure below names `rustup update stable`.
if ! have cargo; then
  say "cargo not found: installing rustup with the minimal stable toolchain"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
  PATH="${HOME:-}/.cargo/bin:$PATH"
  export PATH
  have cargo || die "rustup installed but cargo is still not on PATH; open a new shell and run this script again"
fi

work=$(mktemp -d "${VERIDOC_WORK_DIR:-${TMPDIR:-/tmp}}/veridoc-install.XXXXXX") ||
  die "could not create a build directory under ${VERIDOC_WORK_DIR:-${TMPDIR:-/tmp}}"
trap 'rm -rf "$work"' EXIT INT TERM

say "cloning $SRC_URL${REF:+ at $REF}"
if [ -n "$REF" ]; then
  git clone --quiet --depth 1 --branch "$REF" "$SRC_URL" "$work/veridoc" </dev/null
else
  git clone --quiet --depth 1 "$SRC_URL" "$work/veridoc" </dev/null
fi

say "building the release binary; the first build takes a few minutes"
if ! (cd "$work/veridoc" && cargo build --release --locked -p veridoc-cli); then
  die "cargo build failed; a rust-version error means you need \`rustup update stable\`"
fi

mkdir -p "$INSTALL_DIR" || die "could not create $INSTALL_DIR; point VERIDOC_INSTALL_DIR at a directory on your PATH"
src="$work/veridoc/target/release/veridoc"
[ -f "$src" ] || die "the build produced no veridoc"
cp "$src" "$INSTALL_DIR/veridoc.new" ||
  die "could not write to $INSTALL_DIR; point VERIDOC_INSTALL_DIR at a directory you can write to"
chmod 0755 "$INSTALL_DIR/veridoc.new"
mv -f "$INSTALL_DIR/veridoc.new" "$INSTALL_DIR/veridoc"

hash -r 2>/dev/null || true
say "installed $("$INSTALL_DIR/veridoc" --version) into $INSTALL_DIR"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) say "add it to PATH with:  export PATH=\"$INSTALL_DIR:\$PATH\"";;
esac
found=$(command -v veridoc || true)
if [ -n "$found" ] && [ "$found" != "$INSTALL_DIR/veridoc" ]; then
  say "$INSTALL_DIR is not early enough on PATH: \`veridoc\` still resolves to $found"
fi
say "next: veridoc install-plugin <git-url>  (for example https://github.com/DustinEwan/veridoc-plugin-ts.git)"
