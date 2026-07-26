#!/bin/sh
set -eu

repository="Brobicho/codex-operations-center"
version="${CODEX_OPS_VERSION:-latest}"
install_root="${CODEX_OPS_HOME:-${HOME}/.local/share/codex-ops}"
bin_dir="${CODEX_OPS_BIN_DIR:-${HOME}/.local/bin}"

case "$(uname -s)" in
  Linux) os="unknown-linux-gnu" ;;
  Darwin) os="apple-darwin" ;;
  *) printf '%s\n' "Unsupported operating system: $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  x86_64|amd64) arch="x86_64" ;;
  arm64|aarch64) arch="aarch64" ;;
  *) printf '%s\n' "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

target="${arch}-${os}"
archive="codex-ops-${target}.tar.gz"
checksum_file="codex-ops-${target}.sha256"
if [ "$version" = "latest" ]; then
  release_url="https://github.com/${repository}/releases/latest/download"
else
  release_url="https://github.com/${repository}/releases/download/${version}"
fi
release_url="${CODEX_OPS_RELEASE_URL:-$release_url}"

temporary="$(mktemp -d)"
cleanup() { rm -rf -- "$temporary"; }
trap cleanup EXIT HUP INT TERM

download() {
  source_url="$1"
  destination="$2"
  if command -v curl >/dev/null 2>&1; then
    case "$source_url" in
      https://*) curl --proto '=https' --tlsv1.2 -fsSL "$source_url" -o "$destination" ;;
      *) curl -fsSL "$source_url" -o "$destination" ;;
    esac
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$source_url" -O "$destination"
  else
    printf '%s\n' "curl or wget is required" >&2
    exit 1
  fi
}

printf '%s\n' "Downloading Codex Operations Center for ${target}..."
download "${release_url}/${archive}" "${temporary}/${archive}"
download "${release_url}/${checksum_file}" "${temporary}/${checksum_file}"

expected="$(awk -v archive="$archive" '$2 == archive || $2 == "*" archive {print $1}' "${temporary}/${checksum_file}")"
[ -n "$expected" ] || { printf '%s\n' "Archive checksum is missing" >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${temporary}/${archive}" | awk '{print $1}')"
else
  actual="$(shasum -a 256 "${temporary}/${archive}" | awk '{print $1}')"
fi
[ "$expected" = "$actual" ] || { printf '%s\n' "Checksum verification failed" >&2; exit 1; }

tar -xzf "${temporary}/${archive}" -C "$temporary"
mkdir -p "$install_root" "$bin_dir"
install -m 0755 "${temporary}/codex-ops" "${install_root}/codex-ops"
ln -sfn "${install_root}/codex-ops" "${bin_dir}/codex-ops"

if [ "${CODEX_OPS_SKIP_INTEGRATION:-0}" != "1" ]; then
  "${install_root}/codex-ops" integrate
fi

printf '\n%s\n' "Codex Operations Center installed successfully."
printf '  Executable: %s\n' "${install_root}/codex-ops"
printf '  Launcher:   %s\n' "${bin_dir}/codex-ops"
printf '\nRun: codex-ops doctor\n'
case ":${PATH}:" in
  *":${bin_dir}:"*) ;;
  *) printf 'Add %s to PATH before using the short command.\n' "$bin_dir" ;;
esac
