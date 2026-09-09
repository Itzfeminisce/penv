#!/bin/sh
# penv installer. curl -fsSL https://penv.cloud/install | sh
#
# PENV_VERSION       pin a tag, such as v1.2.3; default is the latest release
# PENV_INSTALL_DIR   where the binary lands; default $HOME/.penv/bin
# PENV_RELEASE_BASE  the address the release is read from; default https://penv.cloud
# PENV_TARGET        install for another machine's triple instead of this one's
# PENV_ALLOW_ROOT    1 installs as root; the default refuses

set -eu

# penv.cloud redirects to whichever host serves the releases, so no repository path lives here.
base=${PENV_RELEASE_BASE:-https://penv.cloud}
base=${base%/}
targets="x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc aarch64-pc-windows-msvc"

say() { printf '%s\n' "$*"; }
die() { printf 'penv: %s\n' "$*" >&2; exit 1; }

if [ "$(id -u 2>/dev/null || echo 1)" = 0 ] && [ "${PENV_ALLOW_ROOT:-}" != 1 ]; then
    die "this installs into a home directory, so run it as the user who will run penv, or set PENV_ALLOW_ROOT=1."
fi

triple=${PENV_TARGET:-}
if [ -n "$triple" ]; then
    case " $targets " in
        *" $triple "*) ;;
        *) die "penv publishes no $triple build. The releases carry $targets." ;;
    esac
else
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Linux) os=unknown-linux-musl ;;
        Darwin) os=apple-darwin ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT)
            die "on Windows run the PowerShell installer: irm https://penv.cloud/install.ps1 | iex"
            ;;
        *) die "no penv build for $os. The releases carry $targets." ;;
    esac
    case "$arch" in
        x86_64 | amd64) arch=x86_64 ;;
        aarch64 | arm64) arch=aarch64 ;;
        *) die "no penv build for $arch on $os. The releases carry $targets." ;;
    esac
    triple=$arch-$os
fi
case "$triple" in
    *windows*) exe=.exe ;;
    *) exe= ;;
esac

if command -v curl >/dev/null 2>&1; then
    # --proto-redir constrains redirects only, so a plain http base still works while a
    # redirect off https cannot walk the download back down to http.
    fetch() { curl -fsSL --proto-redir '=https' "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
    # wget has no per-redirect protocol filter, so this path trusts the redirect chain.
    fetch() { wget -qO "$2" "$1"; }
else
    die "neither curl nor wget is on PATH, and one of them downloads the release."
fi

if command -v sha256sum >/dev/null 2>&1; then
    digest_of() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
    digest_of() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
    die "neither sha256sum nor shasum is on PATH, and the download is verified before it is installed."
fi

cleanup() {
    [ -n "${temp:-}" ] && rm -f "$temp"
    [ -n "${work:-}" ] && rm -rf "$work"
    return 0
}
trap 'cleanup' EXIT
trap 'cleanup; exit 130' INT TERM

work=$(mktemp -d "${TMPDIR:-/tmp}/penv-install.XXXXXX")

tag=${PENV_VERSION:-}
if [ -z "$tag" ]; then
    latest=$base/releases/latest
    fetch "$latest" "$work/release.json" || die "$latest could not be read. Check the network and try again."
    tag=$(tr ',' '\n' <"$work/release.json" |
        sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
    [ -n "$tag" ] || die "the latest release carries no tag_name. Try again later."
fi
# One shape whichever way the tag arrived, and nothing in it that could reach past the asset.
tag=v${tag#v}
case $tag in
    *[!0-9A-Za-z.+_-]*) die "$tag is not a tag such as v1.2.3: a tag carries no slash and no whitespace." ;;
esac

asset=penv-$tag-$triple$exe
sums=penv-$tag-$triple.sha256
download=$base/releases/download/$tag

dir=${PENV_INSTALL_DIR:-$HOME/.penv/bin}
binary=$dir/penv$exe
mkdir -p "$dir" || die "$dir could not be created."
# The install directory is created before the digest passes, and the binary is downloaded
# into it so the rename is atomic and lands over a running penv; the temp file goes on
# every exit path, so a refusal leaves the directory as it found it.
temp=$dir/.penv.$$

say "penv $tag for $triple"
fetch "$download/$asset" "$temp" || die "$download/$asset could not be downloaded."
fetch "$download/$sums" "$work/$sums" || die "$download/$sums could not be downloaded."

# The checksum file covers the archive too, so the raw binary's line is matched whole.
expected=$(sed -n "s/^\([0-9a-fA-F]\{64\}\)[[:space:]][[:space:]]*[*]\{0,1\}$asset\$/\1/p" "$work/$sums" | head -n 1)
[ -n "$expected" ] || die "$sums lists no sha256 digest for $asset."
actual=$(digest_of "$temp")
if [ "$(printf '%s' "$expected" | tr 'A-F' 'a-f')" != "$(printf '%s' "$actual" | tr 'A-F' 'a-f')" ]; then
    die "$asset is not the file $sums names. Nothing was installed."
fi

chmod 755 "$temp"
mv -f "$temp" "$binary" || die "$binary could not be written."

say "installed $binary"
case ":$PATH:" in
    *":$dir:"*)
        say "run: penv init"
        exit 0
        ;;
esac

say ""
case "$(basename "${SHELL:-sh}")" in
    fish) say "put it on your PATH:  fish_add_path \"$dir\"" ;;
    zsh) say "put it on your PATH:  echo 'export PATH=\"$dir:\$PATH\"' >> ~/.zshrc" ;;
    *) say "put it on your PATH:  echo 'export PATH=\"$dir:\$PATH\"' >> ~/.profile" ;;
esac
say "or run it now:        \"$binary\" init"
