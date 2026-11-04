//! The video backend: FFmpeg, linked in.
//!
//! The renderer (`sicompass_ui::video`) has the player, the clock and the audio output.
//! This crate has the decoding: [`FfmpegBackend`] opens a file, and the stream it gives
//! hands out pictures as RGBA and sound as stereo 48 kHz, in the order they were
//! written, which is the order to present them.

use ff::format::Pixel;
use ff::software::scaling::Flags;
use ffmpeg_next as ff;
use sicompass_ui::video::{
    AUDIO_CHANNELS, AUDIO_RATE, AudioChunk, Packet, VideoBackend, VideoFrame, VideoInfo,
    VideoStream,
};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

// vcpkg's static FFmpeg on Windows holds its Media Foundation wrappers, which need these
// two libraries, and ffmpeg-sys-next names neither. Without them the link stops on
// IID_ICodecAPI and IID_IMFTransform.
#[cfg(all(windows, feature = "bundled-ffmpeg"))]
#[link(name = "mfuuid")]
#[link(name = "strmiids")]
unsafe extern "C" {}

/// Make video playable in this process.
pub fn register() {
    if let Err(e) = ff::init() {
        tracing::warn!("video: FFmpeg could not start: {e}");
        return;
    }
    // FFmpeg talks a lot. Its errors reach us as `Err`.
    ff::util::log::set_level(ff::util::log::Level::Error);
    sicompass_ui::video::register_backend(Box::new(FfmpegBackend));
}

pub struct FfmpegBackend;

impl VideoBackend for FfmpegBackend {
    fn open(&self, path: &str, max_side: Option<u32>) -> Result<Box<dyn VideoStream>, String> {
        Ok(Box::new(FfmpegStream::open(path, max_side)?))
    }
}

/// A file FFmpeg can open: an `asset:` URI is written out first, since another library
/// cannot read inside this binary.
fn playable_path(path: &str) -> Result<PathBuf, String> {
    if !sicompass_sdk::assets::is_uri(path) {
        return Ok(PathBuf::from(path));
    }
    let bytes =
        sicompass_sdk::assets::resolve(path).ok_or_else(|| format!("no such asset: {path}"))?;
    let name = path.rsplit('/').next().unwrap_or("video");
    // The same bytes always land in the same file.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for b in path.bytes().chain(bytes.len().to_le_bytes()) {
        hash = (hash ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    let dir = sicompass_sdk::platform::app_cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("video-assets");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{hash:016x}-{name}"));
    if std::fs::metadata(&file).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
        std::fs::write(&file, &*bytes).map_err(|e| e.to_string())?;
    }
    Ok(file)
}

fn seconds(pts: i64, base: ff::Rational) -> Duration {
    if base.denominator() == 0 || pts < 0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(pts as f64 * base.numerator() as f64 / base.denominator() as f64)
}

/// The size to scale to: the video's own, or smaller so the longer side is `max_side`.
/// Even, because the formats it comes from are.
fn scaled_size(width: u32, height: u32, max_side: Option<u32>) -> (u32, u32) {
    let Some(max) = max_side.filter(|m| *m > 0 && *m < width.max(height)) else {
        return (width, height);
    };
    let scale = max as f64 / width.max(height) as f64;
    let even = |v: f64| ((v.round() as u32).max(2)) & !1;
    (even(width as f64 * scale), even(height as f64 * scale))
}

struct AudioPart {
    index: usize,
    decoder: ff::decoder::Audio,
    resampler: Option<ff::software::resampling::Context>,
    base: ff::Rational,
}

pub struct FfmpegStream {
    input: ff::format::context::Input,
    video_index: usize,
    video_base: ff::Rational,
    video: ff::decoder::Video,
    scaler: Option<ff::software::scaling::Context>,
    out_size: (u32, u32),
    audio: Option<AudioPart>,
    ready: VecDeque<Packet>,
    flushed: bool,
    frame_count: u64,
    frame_gap: Duration,
    info: VideoInfo,
}

// SAFETY: FFmpeg's contexts are not thread-safe, but they are not shared either. A stream
// is opened on one thread, moved whole to the thread that decodes it, and used by that
// thread alone, which is what FFmpeg asks for.
unsafe impl Send for FfmpegStream {}

impl FfmpegStream {
    pub fn open(path: &str, max_side: Option<u32>) -> Result<FfmpegStream, String> {
        let file = playable_path(path)?;
        let input = ff::format::input(&file).map_err(|e| format!("{path}: {e}"))?;
        let stream = input
            .streams()
            .best(ff::media::Type::Video)
            .ok_or_else(|| format!("{path} has no video"))?;
        let video_index = stream.index();
        let video_base = stream.time_base();
        let rate = stream.avg_frame_rate();
        let frame_gap = if rate.numerator() > 0 && rate.denominator() > 0 {
            Duration::from_secs_f64(rate.denominator() as f64 / rate.numerator() as f64)
        } else {
            Duration::from_millis(40)
        };
        let video = ff::codec::context::Context::from_parameters(stream.parameters())
            .and_then(|c| c.decoder().video())
            .map_err(|e| format!("{path}: {e}"))?;
        let audio = input.streams().best(ff::media::Type::Audio).and_then(|s| {
            let decoder = ff::codec::context::Context::from_parameters(s.parameters())
                .ok()?
                .decoder()
                .audio()
                .ok()?;
            Some(AudioPart {
                index: s.index(),
                decoder,
                resampler: None,
                base: s.time_base(),
            })
        });
        let (width, height) = (video.width(), video.height());
        let info = VideoInfo {
            width,
            height,
            duration: (input.duration() > 0)
                .then(|| Duration::from_micros(input.duration() as u64)),
            has_audio: audio.is_some(),
        };
        Ok(FfmpegStream {
            input,
            video_index,
            video_base,
            video,
            scaler: None,
            out_size: scaled_size(width, height, max_side),
            audio,
            ready: VecDeque::new(),
            flushed: false,
            frame_count: 0,
            frame_gap,
            info,
        })
    }

    /// Turn the frames the video decoder has ready into pictures.
    fn drain_video(&mut self) {
        let mut decoded = ff::frame::Video::empty();
        while self.video.receive_frame(&mut decoded).is_ok() {
            let (w, h) = self.out_size;
            let scaler = match self.scaler.as_mut() {
                Some(s) => s,
                None => {
                    match ff::software::scaling::Context::get(
                        decoded.format(),
                        decoded.width(),
                        decoded.height(),
                        Pixel::RGBA,
                        w,
                        h,
                        Flags::BILINEAR,
                    ) {
                        Ok(s) => self.scaler.insert(s),
                        Err(_) => return,
                    }
                }
            };
            let mut rgba = ff::frame::Video::empty();
            if scaler.run(&decoded, &mut rgba).is_err() {
                continue;
            }
            let stride = rgba.stride(0);
            let data = rgba.data(0);
            let row = (w * 4) as usize;
            let mut pixels = Vec::with_capacity(row * h as usize);
            for y in 0..h as usize {
                let start = y * stride;
                match data.get(start..start + row) {
                    Some(line) => pixels.extend_from_slice(line),
                    None => break,
                }
            }
            if pixels.len() != row * h as usize {
                continue;
            }
            let pts = match decoded.timestamp() {
                Some(t) => seconds(t, self.video_base),
                None => self.frame_gap * self.frame_count as u32,
            };
            self.frame_count += 1;
            self.ready.push_back(Packet::Video(VideoFrame {
                width: w,
                height: h,
                rgba: pixels,
                pts,
            }));
        }
    }

    /// Turn the sound the audio decoder has ready into stereo 48 kHz.
    fn drain_audio(&mut self) {
        let Some(audio) = self.audio.as_mut() else {
            return;
        };
        let mut decoded = ff::frame::Audio::empty();
        while audio.decoder.receive_frame(&mut decoded).is_ok() {
            if decoded.channel_layout().is_empty() {
                decoded.set_channel_layout(ff::ChannelLayout::default(decoded.channels() as i32));
            }
            let resampler = match audio.resampler.as_mut() {
                Some(r) => r,
                None => {
                    match ff::software::resampling::Context::get(
                        decoded.format(),
                        decoded.channel_layout(),
                        decoded.rate(),
                        ff::format::Sample::I16(ff::format::sample::Type::Packed),
                        ff::ChannelLayout::STEREO,
                        AUDIO_RATE,
                    ) {
                        Ok(r) => audio.resampler.insert(r),
                        Err(_) => return,
                    }
                }
            };
            let mut out = ff::frame::Audio::empty();
            if resampler.run(&decoded, &mut out).is_err() || out.samples() == 0 {
                continue;
            }
            let bytes = &out.data(0)[..out.samples() * AUDIO_CHANNELS as usize * 2];
            let samples: Vec<i16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
            let pts = decoded
                .timestamp()
                .map_or(Duration::ZERO, |t| seconds(t, audio.base));
            self.ready
                .push_back(Packet::Audio(AudioChunk { samples, pts }));
        }
    }
}

impl VideoStream for FfmpegStream {
    fn info(&self) -> VideoInfo {
        self.info.clone()
    }

    fn next(&mut self) -> Packet {
        loop {
            if let Some(packet) = self.ready.pop_front() {
                return packet;
            }
            if self.flushed {
                return Packet::End;
            }
            let mut packet = ff::Packet::empty();
            match packet.read(&mut self.input) {
                Ok(()) => {
                    let index = packet.stream();
                    if index == self.video_index {
                        if self.video.send_packet(&packet).is_ok() {
                            self.drain_video();
                        }
                    } else if let Some(audio) = self.audio.as_mut()
                        && audio.index == index
                        && audio.decoder.send_packet(&packet).is_ok()
                    {
                        self.drain_audio();
                    }
                }
                // The end, or something unreadable, which is the same to a viewer.
                Err(_) => {
                    let _ = self.video.send_eof();
                    self.drain_video();
                    if let Some(audio) = self.audio.as_mut() {
                        let _ = audio.decoder.send_eof();
                    }
                    self.drain_audio();
                    self.flushed = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip() -> String {
        format!(
            "{}/src/tests/fixtures/video/clip.mp4",
            env!("CARGO_MANIFEST_DIR").trim_end_matches("/src")
        )
    }

    #[test]
    fn a_clip_opens_and_says_what_it_is() {
        let stream = FfmpegStream::open(&clip(), None).expect("opens");
        let info = stream.info();
        assert_eq!((info.width, info.height), (160, 90));
        assert!(info.has_audio);
        let duration = info.duration.expect("a duration");
        assert!(
            duration > Duration::from_millis(800) && duration < Duration::from_millis(1300),
            "{duration:?}"
        );
    }

    #[test]
    fn a_clip_decodes_to_all_its_pictures_and_its_sound() {
        let mut stream = FfmpegStream::open(&clip(), None).unwrap();
        let (mut frames, mut samples, mut last_video) = (0, 0usize, Duration::ZERO);
        loop {
            match stream.next() {
                Packet::Video(f) => {
                    assert_eq!((f.width, f.height, f.rgba.len()), (160, 90, 160 * 90 * 4));
                    assert!(f.pts >= last_video, "pictures come in order");
                    last_video = f.pts;
                    frames += 1;
                }
                Packet::Audio(a) => samples += a.samples.len(),
                Packet::End => break,
            }
        }
        assert_eq!(frames, 25, "one second at 25 per second, the last included");
        // About a second of stereo at 48 kHz, give or take what the encoder pads.
        let per_second = (AUDIO_RATE * AUDIO_CHANNELS) as usize;
        assert!(
            (per_second * 9 / 10..per_second * 12 / 10).contains(&samples),
            "{samples}"
        );
        assert_eq!(stream.next(), Packet::End, "and again after the end");
    }

    #[test]
    fn the_pictures_are_not_all_black() {
        let mut stream = FfmpegStream::open(&clip(), None).unwrap();
        let first = loop {
            if let Packet::Video(f) = stream.next() {
                break f;
            }
        };
        assert!(
            first
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] > 30 || p[1] > 30 || p[2] > 30)
        );
        assert!(first.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    }

    #[test]
    fn a_thumbnail_is_made_no_bigger_than_asked() {
        let mut stream = FfmpegStream::open(&clip(), Some(64)).unwrap();
        let first = loop {
            if let Packet::Video(f) = stream.next() {
                break f;
            }
        };
        assert_eq!((first.width, first.height), (64, 36));
        assert_eq!(first.rgba.len(), 64 * 36 * 4);
    }

    #[test]
    fn sizes_scale_evenly_and_never_up() {
        assert_eq!(scaled_size(1920, 1080, Some(256)), (256, 144));
        assert_eq!(scaled_size(1080, 1920, Some(256)), (144, 256));
        assert_eq!(scaled_size(160, 90, Some(1000)), (160, 90));
        assert_eq!(scaled_size(160, 90, None), (160, 90));
        assert_eq!(scaled_size(10, 4, Some(3)), (2, 2));
    }

    #[test]
    fn a_file_that_is_not_a_video_does_not_open() {
        assert!(FfmpegStream::open("/no/such/clip.mp4", None).is_err());
        let dir = std::env::temp_dir().join(format!("video-ffmpeg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("junk.mp4");
        std::fs::write(&junk, b"not a video at all").unwrap();
        assert!(FfmpegStream::open(&junk.to_string_lossy(), None).is_err());
    }

    #[test]
    fn the_backend_is_the_one_the_renderer_asks() {
        register();
        assert!(sicompass_ui::video::available());
        let frame = sicompass_ui::video::first_frame(&clip(), 64).expect("a first frame");
        assert_eq!((frame.width, frame.height), (64, 36));
        assert_eq!(sicompass_ui::video::dimensions(&clip()), Some((160, 90)));
    }

    #[test]
    fn an_asset_is_written_out_where_ffmpeg_can_read_it() {
        sicompass_sdk::assets::register_bytes(
            "video-test",
            "tiny.mp4",
            include_bytes!("tests/fixtures/video/clip.mp4"),
        );
        let path = playable_path("asset:video-test/tiny.mp4").unwrap();
        assert!(path.is_file());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 7522);
        let again = playable_path("asset:video-test/tiny.mp4").unwrap();
        assert_eq!(path, again, "the same bytes land in the same file");
        assert!(FfmpegStream::open("asset:video-test/tiny.mp4", None).is_ok());
    }
}
