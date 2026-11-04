# Video

A `<video>` in a row plays inside the window, like a picture: Enter or `d` on it
opens the big view and starts it, Enter pauses and resumes, Escape stops, and Left
and Right move over the pictures and videos of the row. A video with no `poster`
shows its first picture in the row.

## Where it lives

- **The player is in the renderer** (`sicompass-ui`, `src/video.rs`): a decoder
  thread filling a bounded queue, a clock that pauses, sound queued a little ahead
  of it, and a texture updated in place for each picture. It has no decoder.
- **The decoder is the application's** (`src/video_ffmpeg.rs`): it links FFmpeg
  through `ffmpeg-next` and registers itself with `sicompass_ui::video::register_backend`.
  The login screen shares the renderer and registers nothing, so it links none of this.

## How FFmpeg gets into each build

Every build plays video. There is no switch. The only choice is how FFmpeg links:

- **Nix and development:** dynamically. The dev shell and the package provide FFmpeg and
  libclang (the bindings are generated with bindgen), so a plain `cargo build` works.
- **Release packages and CI:** statically, with the `bundled-ffmpeg` feature, so the one
  standalone binary stays standalone and a user has nothing to install. Before cargo
  runs, `scripts/setup-ffmpeg.sh` builds a decode-only FFmpeg from a pinned release, and
  `scripts/setup-ffmpeg.ps1` takes vcpkg's static build on Windows. Both need `FFMPEG_DIR`
  or `VCPKG_ROOT` in the environment they print, which `ci.yml`, `native-packages.yml` and
  `parts/build-setup.yml` arrange.

Moving FFmpeg's version means changing `FFMPEG_VERSION` in the script and the note at the
top of `licenses/LICENSE-FFmpeg.txt`.

## Licensing

FFmpeg is LGPL 2.1 or later as built here: `--enable-gpl` and `--enable-nonfree` are never
passed. Its text is `licenses/LICENSE-FFmpeg.txt`, in the same hand-kept lists as the font
licenses. `cargo about` only sees the Rust bindings (`ffmpeg-next` and `ffmpeg-sys-next`,
WTFPL).

**Patents are a separate question from the licence.** The build decodes H.264 and HEVC, which
are patent-encumbered in some countries. The project owner decided on 2026-11-04 to ship them
anyway, because most `.mp4` files need them. That is a decision, not legal advice, and nobody
qualified has reviewed it. To switch them off, add `--disable-decoder=h264,hevc` to the
script (and handle the Windows build too).

## Sound

Sound is stereo 48 kHz, signed 16-bit, and goes to SDL's default playback device. A
computer with no audio device plays the picture without sound.
