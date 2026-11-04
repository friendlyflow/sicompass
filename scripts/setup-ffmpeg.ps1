# Provide the FFmpeg the Windows package links statically, and append the
# environment ffmpeg-sys-next needs to $GITHUB_ENV.
#
# See scripts/setup-ffmpeg.sh for why FFmpeg is linked into the binary. Windows
# takes vcpkg's build instead of running FFmpeg's configure, which wants MSYS.
#
#   - The x64-windows-static triplet is the one setup-freetype.ps1 uses, and for the
#     same reason: it matches cargo-dist's `msvc-crt-static` default (/MT).
#   - `core` drops the port's default avdevice and avfilter, and asks for no external
#     codec library, so the result stays LGPL and small.
#   - ffmpeg-sys-next finds it through the vcpkg crate, which needs VCPKG_ROOT and
#     picks the triplet from VCPKGRS_TRIPLET. A missing one fails the build outright.
#   - bindgen finds libclang in the LLVM preinstalled on GitHub's windows runners.

$ErrorActionPreference = "Stop"

$triplet = "x64-windows-static"
$vcpkg = $env:VCPKG_INSTALLATION_ROOT
if (-not $vcpkg) { $vcpkg = "C:\vcpkg" }

Write-Host "Using vcpkg at $vcpkg (triplet $triplet)"
& "$vcpkg\vcpkg.exe" install "ffmpeg[core,avcodec,avformat,swresample,swscale]:$triplet"
if ($LASTEXITCODE -ne 0) { throw "vcpkg install ffmpeg:$triplet failed" }

if (-not (Test-Path "$vcpkg\installed\$triplet\lib\avcodec.lib")) {
    throw "avcodec.lib not found under $vcpkg\installed\$triplet\lib"
}

$vars = @("VCPKG_ROOT=$vcpkg", "VCPKGRS_TRIPLET=$triplet")
foreach ($v in $vars) {
    Write-Host "  $v"
    if ($env:GITHUB_ENV) { Add-Content -Path $env:GITHUB_ENV -Value $v }
}
