#!/bin/sh
# Install a checksum-verified release of alveflo/terminal-news.
set -eu

die() { printf 'terminapp: %s\n' "$*" >&2; exit 1; }

main() {
    for command in curl tar mktemp awk; do
        command -v "$command" >/dev/null 2>&1 || die "Required command not found: $command"
    done
    case "$(uname -s)" in
        Linux) platform=unknown-linux-musl ;;
        Darwin) platform=apple-darwin ;;
        *) die "Supported platforms: Linux, macOS, and Linux under WSL" ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64) architecture=x86_64 ;;
        arm64|aarch64) architecture=aarch64 ;;
        *) die "Supported architectures: x86_64 and ARM64" ;;
    esac
    archive="terminapp-$architecture-$platform.tar.gz"
    release=${TERMINAPP_VERSION:-latest}
    base=https://github.com/alveflo/terminal-news/releases
    if [ "$release" = latest ]; then
        base="$base/latest/download"
    else
        case "$release" in
            *[!a-zA-Z0-9.-]*|'') die "Invalid release version" ;;
            v[0-9]*) ;;
            *) die "Version must look like v0.1.0" ;;
        esac
        base="$base/download/$release"
    fi
    destination=${TERMINAPP_INSTALL_DIR:-"$HOME/.local/bin"}
    [ -n "$destination" ] || die "Installation directory must not be empty"
    temp_dir=$(mktemp -d)
    trap 'rm -rf "$temp_dir"' EXIT
    trap 'exit 1' HUP INT TERM
    printf 'Downloading terminapp (%s, %s)…\n' "$release" "$architecture-$platform"
    curl --fail --silent --show-error --location --retry 3 --connect-timeout 15 --max-time 180 \
        --proto '=https' --tlsv1.2 "$base/$archive" -o "$temp_dir/$archive" || die "Download failed; check that the release exists"
    curl --fail --silent --show-error --location --retry 3 --connect-timeout 15 --max-time 60 \
        --proto '=https' --tlsv1.2 "$base/SHA256SUMS" -o "$temp_dir/SHA256SUMS" || die "Could not download release checksums"
    awk -v file="$archive" '$2 == file { print }' "$temp_dir/SHA256SUMS" > "$temp_dir/checksum"
    [ "$(wc -l < "$temp_dir/checksum" | tr -d ' ')" = 1 ] || die "Missing or duplicate checksum for $archive"
    if command -v sha256sum >/dev/null 2>&1; then
        (cd "$temp_dir" && sha256sum -c checksum) || die "Checksum verification failed"
    elif command -v shasum >/dev/null 2>&1; then
        (cd "$temp_dir" && shasum -a 256 -c checksum) || die "Checksum verification failed"
    else
        die "Install sha256sum or shasum to verify the download"
    fi
    # Extract only the expected file; never run binaries from an unverified archive.
    tar -xzf "$temp_dir/$archive" -C "$temp_dir" terminapp
    [ -f "$temp_dir/terminapp" ] && [ ! -L "$temp_dir/terminapp" ] || die "Release contains no regular terminapp binary"
    mkdir -p "$destination"
    # Rename on the destination filesystem so an interrupted upgrade leaves the old binary intact.
    install_temp=$(mktemp "$destination/.terminapp.XXXXXX")
    trap 'rm -rf "$temp_dir"; rm -f "$install_temp"' EXIT
    cp "$temp_dir/terminapp" "$install_temp"
    chmod 755 "$install_temp"
    mv -f "$install_temp" "$destination/terminapp"
    printf 'Installed %s/terminapp\n' "$destination"
    case ":$PATH:" in
        *":$destination:"*) printf 'Run: terminapp\n' ;;
        *) printf 'Add %s to PATH, then run terminapp.\n' "$destination" ;;
    esac
}

main "$@"
