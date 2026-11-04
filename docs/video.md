# Video

A `<video>` in a row plays inside the window, like a picture: Enter or `d` on it
opens the big view and starts it, Space pauses and resumes, Escape stops, and Left
and Right move over the pictures and videos of the row. A video with no `poster`
shows its first picture in the row.

## Where it lives

- **The player is in the renderer** (`sicompass-ui`, `src/video.rs`): a decoder
  thread filling a bounded queue, a clock that pauses, sound queued a little ahead
  of it, and a texture updated in place for each picture. It has no decoder.
- **The decoder is the application's** (`src/video_ffmpeg.rs`): it links FFmpeg
  through `ffmpeg-next` and registers itself with `sicompass_ui::video::register_backend`.
  The login screen shares the renderer and registers nothing, so it links none of this.

## The `video` feature

FFmpeg is linked only with `--features video`. It is **off by default**, because
the release pipelines do not have the FFmpeg libraries or libclang (the bindings
are generated with bindgen), and because a dynamic link would make the release
archives depend on libraries the user may not have.

- **Nix:** the dev shell and the package provide FFmpeg and the bindgen hook, and the
  package builds with the feature. `nix develop -c cargo build --features video`.
- **A build without it** shows a video's poster or first picture, and says on Enter that
  this version cannot play video. Nothing else changes.
- **Before the release archives can have it**, three things have to be decided: how to
  get FFmpeg onto the four CI runners (a system package, or `ffmpeg-next`'s `build`
  feature, which compiles it); whether to link it statically so the archives stay
  standalone; and what H.264 and HEVC decoding means for redistribution. `ffmpeg-next`
  and `ffmpeg-sys-next` are under the WTFPL, which `about.toml` will have to accept too.

## Sound

Sound is stereo 48 kHz, signed 16-bit, and goes to SDL's default playback device. A
computer with no audio device plays the picture without sound.
