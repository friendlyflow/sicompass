# Make a PNG-capable FreeType available to freetype-sys on Windows, and append
# the environment it needs to $GITHUB_ENV.
#
# See scripts/setup-freetype.sh for why this is necessary at all. The short
# version: NotoColorEmoji's strikes are PNG bitmaps, freetype-sys falls back to
# a vendored build with PNG support switched off when pkg-config finds no
# system FreeType, and that fallback returns zero-sized bitmaps rather than an
# error, so colour emoji silently vanish.
#
# Windows has no system FreeType and no pkg-config, so it has always taken that
# fallback. This installs both.
#
#   - vcpkg is preinstalled on GitHub's windows runners and its freetype port
#     enables PNG by default.
#   - The x64-windows-static triplet matches cargo-dist's `msvc-crt-static`
#     default, which links the CRT statically (/MT). Using the -md triplet here
#     would reintroduce exactly the /MT vs /MD mismatch that `no-sdl-libc`
#     exists to work around for SDL.
#   - pkgconfiglite provides the pkg-config binary the pkg-config crate shells
#     out to. Without it freetype-sys cannot detect anything, however well
#     installed FreeType is.

$ErrorActionPreference = "Stop"

$triplet = "x64-windows-static"
$vcpkg = $env:VCPKG_INSTALLATION_ROOT
if (-not $vcpkg) { $vcpkg = "C:\vcpkg" }

Write-Host "Using vcpkg at $vcpkg (triplet $triplet)"
& "$vcpkg\vcpkg.exe" install "freetype:$triplet"
if ($LASTEXITCODE -ne 0) { throw "vcpkg install freetype:$triplet failed" }

if (-not (Get-Command pkg-config -ErrorAction SilentlyContinue)) {
    Write-Host "Installing pkgconfiglite"
    choco install pkgconfiglite -y --no-progress
    if ($LASTEXITCODE -ne 0) { throw "choco install pkgconfiglite failed" }
    # choco puts it on PATH for later steps, but not this process.
    $env:PATH = "$env:PATH;C:\ProgramData\chocolatey\bin"
}

$pcDir = "$vcpkg\installed\$triplet\lib\pkgconfig"
if (-not (Test-Path "$pcDir\freetype2.pc")) {
    Write-Host "Contents of $vcpkg\installed\$triplet\lib :"
    Get-ChildItem "$vcpkg\installed\$triplet\lib" -ErrorAction SilentlyContinue | Format-Table
    throw "freetype2.pc not found at $pcDir"
}

# Fail here rather than letting the build quietly take the vendored path and
# produce a binary with no colour emoji, which is what this script exists to
# prevent. freetype2.pc carries a libtool version, so the floor reads much
# higher than the release number it corresponds to.
$env:PKG_CONFIG_PATH = $pcDir
$env:PKG_CONFIG_ALLOW_SYSTEM_LIBS = "1"
$env:PKG_CONFIG_ALLOW_SYSTEM_CFLAGS = "1"
$found = (& pkg-config --modversion freetype2) 2>&1
if ($LASTEXITCODE -ne 0) {
    throw "pkg-config could not read freetype2 from $pcDir : $found"
}
Write-Host "freetype2.pc reports $found (floor 24.3.18)"
& pkg-config --atleast-version=24.3.18 freetype2
if ($LASTEXITCODE -ne 0) {
    throw "freetype2.pc reports $found, below the 24.3.18 floor freetype-sys checks"
}

# The pkg-config crate refuses to use system paths for a cross-ish target
# unless told to, and every MSVC build looks cross-ish to it.
$vars = @(
    "PKG_CONFIG_PATH=$pcDir",
    "PKG_CONFIG_ALLOW_SYSTEM_LIBS=1",
    "PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1",
    # freetype2.pc lists its dependencies under Libs.private, which pkg-config
    # only emits for a static link.
    "PKG_CONFIG_ALL_STATIC=1"
)

Write-Host "freetype2.pc found, exporting:"
foreach ($v in $vars) {
    Write-Host "  $v"
    if ($env:GITHUB_ENV) { Add-Content -Path $env:GITHUB_ENV -Value $v }
}
