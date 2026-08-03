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

# `--modversion` succeeding proves very little: freetype-sys calls
# `pkg_config::Config::find()`, which runs `--libs --cflags` and fails if any
# entry in Requires or Requires.private cannot be resolved. That failure is
# swallowed and the build quietly takes the vendored path instead, so probe it
# here where the error is visible.
Write-Host "Available .pc files in ${pcDir}:"
Get-ChildItem $pcDir -Filter *.pc | ForEach-Object { Write-Host "  $($_.Name)" }

$libs = (& pkg-config --print-errors --libs --cflags freetype2) 2>&1
$dynamicOk = ($LASTEXITCODE -eq 0)
Write-Host "pkg-config --libs --cflags          -> exit $LASTEXITCODE : $libs"

$libsStatic = (& pkg-config --print-errors --static --libs --cflags freetype2) 2>&1
$staticOk = ($LASTEXITCODE -eq 0)
Write-Host "pkg-config --static --libs --cflags -> exit $LASTEXITCODE : $libsStatic"

# The one that actually matters. `atleast_version` makes the pkg-config crate
# append the constraint as a *second, space-containing argument* next to the
# package name:
#
#     pkg-config --libs --cflags freetype2 "freetype2 >= 24.3.18"
#
# Probing without it, as above, proves nothing: those succeed today and the
# build still falls back. Older pkg-config implementations mishandle that
# argument form, and freetype-sys swallows the resulting error.
function Test-Constrained {
    param([string]$Exe)
    $out = (& $Exe --print-errors --libs --cflags freetype2 "freetype2 >= 24.3.18") 2>&1
    $ok = ($LASTEXITCODE -eq 0)
    Write-Host "  $Exe (version-constrained) -> exit $LASTEXITCODE : $out"
    return $ok
}

$pkgConfigExe = (Get-Command pkg-config).Source
Write-Host "pkg-config in use: $pkgConfigExe"
Write-Host "  --version: $((& pkg-config --version) 2>&1)"
Write-Host "Probing exactly what freetype-sys runs:"
$constrainedOk = Test-Constrained $pkgConfigExe

if (-not $constrainedOk) {
    # vcpkg ships pkgconf, a modern reimplementation, and its own ports rely on
    # it, so it is already vendored rather than being another thing to install.
    Write-Host "Falling back to vcpkg's pkgconf"
    & "$vcpkg\vcpkg.exe" install "pkgconf:$triplet"
    $pkgconf = Get-ChildItem -Path $vcpkg -Filter pkgconf.exe -Recurse -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if (-not $pkgconf) { throw "pkgconf.exe not found after installing it" }
    Write-Host "pkgconf at $($pkgconf.FullName)"
    if (Test-Constrained $pkgconf.FullName) {
        $constrainedOk = $true
        $pkgConfigExe = $pkgconf.FullName
    }
}

if (-not $constrainedOk) {
    throw "No pkg-config implementation can satisfy the version-constrained query freetype-sys makes, so it will fall back to its vendored build with no PNG support"
}

# The pkg-config crate strips paths it considers system defaults unless told
# otherwise, which on a vcpkg prefix throws away exactly the flags we need.
$vars = @(
    "PKG_CONFIG_PATH=$pcDir",
    "PKG_CONFIG_ALLOW_SYSTEM_LIBS=1",
    "PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1",
    # THIS LINE IS THE FIX. Do not drop it on the grounds that pkg-config is
    # already on PATH.
    #
    # The only pkg-config on a GitHub windows runner is Strawberry Perl's
    # `C:\Strawberry\perl\bin\pkg-config.bat`. Rust's `Command::new(
    # "pkg-config")` resolves names against PATH looking for an executable and
    # does not apply PATHEXT, so it never finds a bare `.bat` and the crate's
    # `find()` fails before running anything. Given an explicit path ending in
    # `.bat`, Rust routes it through cmd.exe and it works.
    #
    # This is why every probe above passed while the build still fell back:
    # PowerShell resolves the `.bat` via PATHEXT, and Rust does not, so the
    # shell and the build script disagreed about whether pkg-config existed.
    "PKG_CONFIG=$pkgConfigExe"
)

# PKG_CONFIG_ALL_STATIC is deliberately NOT set. It makes the pkg-config crate
# pass `--static`, which additionally resolves every entry in Requires.private
# (brotli, bzip2, libpng, zlib for vcpkg's freetype). One unresolvable entry
# there fails the whole probe, and freetype-sys swallows that and silently
# falls back to its vendored build. Only add it back if the static probe above
# is shown to succeed.
if ($staticOk -and -not $dynamicOk) {
    Write-Host "Only the static probe works, so enabling PKG_CONFIG_ALL_STATIC"
    $vars += "PKG_CONFIG_ALL_STATIC=1"
}

Write-Host "freetype2.pc found, exporting:"
foreach ($v in $vars) {
    Write-Host "  $v"
    if ($env:GITHUB_ENV) { Add-Content -Path $env:GITHUB_ENV -Value $v }
}
