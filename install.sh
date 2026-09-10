#!/bin/sh
# Sift installer.
#
#   curl -fsSL https://raw.githubusercontent.com/sergiocardoso/sift/main/install.sh | sh
#
# Downloads the latest sift release for your OS/architecture from GitHub
# Releases, verifies its SHA256 checksum, and installs it to
# ~/.local/bin/sift (no sudo, ever). Then checks whether `ffprobe` is on
# your PATH; if it isn't, this script only ever *tells* you the right
# command to install it for your system, and offers to run that command
# for you IF this is an interactive terminal AND you say yes — never
# silently, never non-interactively.
#
# POSIX sh only (no bashisms), so it runs under any /bin/sh.

set -eu

REPO="sergiocardoso/sift"
BIN_NAME="sift"
INSTALL_DIR="${SIFT_INSTALL_DIR:-$HOME/.local/bin}"

say() {
  printf '%s\n' "$1"
}

die() {
  printf 'Error: %s\n' "$1" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "'$1' is required but was not found on PATH."
}

need_cmd curl
need_cmd tar

# --------------------------------------------------------------- target

os_name="$(uname -s)"
arch_name="$(uname -m)"

case "$os_name" in
  Linux) os_part="unknown-linux-gnu" ;;
  Darwin) os_part="apple-darwin" ;;
  *) die "unsupported operating system: $os_name (sift currently ships Linux and macOS binaries only)" ;;
esac

case "$arch_name" in
  x86_64 | amd64) arch_part="x86_64" ;;
  arm64 | aarch64) arch_part="aarch64" ;;
  *) die "unsupported architecture: $arch_name" ;;
esac

target="${arch_part}-${os_part}"

# ------------------------------------------------------------ resolve tag

say "Resolving the latest sift release..."
api_url="${SIFT_API_URL:-https://api.github.com/repos/${REPO}/releases/latest}"
api_response="$(curl -fsSL "$api_url")" || die "could not resolve the latest sift release from $api_url"
tag="$(printf '%s\n' "$api_response" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
[ -n "$tag" ] || die "could not resolve the latest sift release from $api_url"
say "Latest release: $tag"

archive="${BIN_NAME}-${target}.tar.gz"
checksum_file="${BIN_NAME}-${target}.sha256"
# SIFT_RELEASE_BASE_URL lets this script (or a fork) point at a mirror,
# or lets tests point at a local fake release server.
base_url="${SIFT_RELEASE_BASE_URL:-https://github.com/${REPO}/releases/download/${tag}}"

# ---------------------------------------------------------- download+verify

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT INT TERM

say "Downloading ${archive}..."
curl -fsSL -o "${tmp_dir}/${archive}" "${base_url}/${archive}" \
  || die "failed to download ${base_url}/${archive} (is $target a published target for $tag?)"
curl -fsSL -o "${tmp_dir}/${checksum_file}" "${base_url}/${checksum_file}" \
  || die "failed to download the checksum file ${base_url}/${checksum_file}"

say "Verifying checksum..."
(
  cd "$tmp_dir"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$checksum_file"
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "$checksum_file"
  else
    die "neither 'sha256sum' nor 'shasum' is available to verify the download"
  fi
) || die "checksum verification failed — refusing to install a file that doesn't match its published checksum"

# -------------------------------------------------------------- install

mkdir -p "$INSTALL_DIR"
tar -xzf "${tmp_dir}/${archive}" -C "$tmp_dir" "$BIN_NAME"
chmod +x "${tmp_dir}/${BIN_NAME}"
mv "${tmp_dir}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
say "Installed ${INSTALL_DIR}/${BIN_NAME}"

case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    say ""
    say "${INSTALL_DIR} is not on your PATH. Add this to your shell profile:"
    say "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac

# ---------------------------------------------------- optional: ffprobe

say ""
if command -v ffprobe >/dev/null 2>&1; then
  say "ffprobe found — sift's 'video' organize strategy will use it for richer metadata."
else
  say "ffprobe was not found. It's entirely optional: sift's 'video' organize"
  say "strategy works without it (MP4/MOV only), and uses it automatically for"
  say "more formats and fields (duration, fps, ...) when it's installed."
  say ""

  install_cmd=""
  if command -v apt-get >/dev/null 2>&1; then
    install_cmd="sudo apt-get update && sudo apt-get install -y ffmpeg"
  elif command -v dnf >/dev/null 2>&1; then
    install_cmd="sudo dnf install -y ffmpeg"
  elif command -v pacman >/dev/null 2>&1; then
    install_cmd="sudo pacman -S --noconfirm ffmpeg"
  elif command -v brew >/dev/null 2>&1; then
    install_cmd="brew install ffmpeg"
  fi

  if [ -n "$install_cmd" ]; then
    say "To install it: $install_cmd"
    # Only ever offer to run this ourselves in a real interactive
    # terminal, and only after an explicit yes — never in a
    # non-interactive `curl | sh` pipe, and never silently.
    if [ -t 0 ] && [ -t 1 ]; then
      printf "Install ffmpeg now? [y/N] "
      read -r answer
      case "$answer" in
        y | Y | yes | YES)
          sh -c "$install_cmd"
          ;;
        *)
          say "Skipped. You can run the command above any time."
          ;;
      esac
    else
      say "(Running non-interactively, so not attempting this automatically.)"
    fi
  else
    say "Install ffmpeg with your system's package manager to enable it."
  fi
fi

say ""
say "Done. Run 'sift --help' to get started."
