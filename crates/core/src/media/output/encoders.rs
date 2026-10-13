//! The video and audio encoders Wirecatch drives, the order they are tried in, and the short test that tells whether a hardware encoder works here.

#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::ffi::OsString;

use wirecatch_streams::ffmpeg::preamble;

use super::{AudioCodec, VideoCodec};

/// A video encoder Wirecatch knows how to drive.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Encoder {
    /// ffmpeg's name for it.
    pub name: &'static str,
    /// It uses the graphics chip (or a media engine), so it is probed before use.
    pub hardware: bool,
    /// Options before the inputs (a device to open).
    pub global: &'static [&'static str],
    /// The filter that brings the frames into a form it takes.
    pub filter: &'static str,
    /// Its own options (speed and quality presets).
    pub options: &'static [&'static str],
    /// Operating systems it exists on.
    pub systems: &'static [&'static str],
    /// The media add-on's ffmpeg (an LGPL build) has it: the encoder used when ffmpeg cannot be asked which it has.
    pub addon_default: bool,
}

/// The VA-API device opened for encoding: the first render node, the graphics chip on a machine with one.
const VAAPI_DEVICE: &[&str] = &["-vaapi_device", "/dev/dri/renderD128"];
/// Every operating system Wirecatch runs on.
const ANY: &[&str] = &["windows", "linux", "macos"];

/// H.264 encoders, in the order they are tried: hardware first (NVIDIA, Intel, AMD, then the system's own Media Foundation on Windows or VA-API on Linux), then software. The media add-on's ffmpeg is an LGPL build without libx264, so its software encoder is `OpenH264`; a build with libx264 uses that instead.
pub(crate) const H264_ENCODERS: &[Encoder] = &[
    Encoder {
        name: "h264_nvenc",
        hardware: true,
        global: &[],
        filter: "format=yuv420p",
        options: &["-preset", "p5"],
        systems: &["windows", "linux"],
        addon_default: false,
    },
    Encoder {
        name: "h264_qsv",
        hardware: true,
        global: &[],
        filter: "format=nv12",
        options: &["-preset", "medium"],
        systems: &["windows", "linux"],
        addon_default: false,
    },
    Encoder {
        name: "h264_amf",
        hardware: true,
        global: &[],
        filter: "format=nv12",
        options: &["-quality", "balanced"],
        systems: &["windows", "linux"],
        addon_default: false,
    },
    Encoder {
        name: "h264_mf",
        hardware: true,
        global: &[],
        filter: "format=nv12",
        options: &["-hw_encoding", "1"],
        systems: &["windows"],
        addon_default: false,
    },
    Encoder {
        name: "h264_vaapi",
        hardware: true,
        global: VAAPI_DEVICE,
        filter: "format=nv12,hwupload",
        options: &[],
        systems: &["linux"],
        addon_default: false,
    },
    Encoder {
        name: "libx264",
        hardware: false,
        global: &[],
        filter: "format=yuv420p",
        options: &["-preset", "veryfast"],
        systems: ANY,
        addon_default: false,
    },
    Encoder {
        name: "libopenh264",
        hardware: false,
        global: &[],
        filter: "format=yuv420p",
        options: &[
            "-profile:v",
            "high",
            "-coder",
            "cabac",
            "-rc_mode",
            "bitrate",
        ],
        systems: ANY,
        addon_default: true,
    },
];

/// VP9 encoders (for WebM), in the order they are tried.
pub(crate) const VP9_ENCODERS: &[Encoder] = &[
    Encoder {
        name: "vp9_qsv",
        hardware: true,
        global: &[],
        filter: "format=nv12",
        options: &[],
        systems: &["windows", "linux"],
        addon_default: false,
    },
    Encoder {
        name: "vp9_vaapi",
        hardware: true,
        global: VAAPI_DEVICE,
        filter: "format=nv12,hwupload",
        options: &[],
        systems: &["linux"],
        addon_default: false,
    },
    Encoder {
        name: "libvpx-vp9",
        hardware: false,
        global: &[],
        filter: "format=yuv420p",
        options: &[
            "-deadline",
            "good",
            "-cpu-used",
            "5",
            "-row-mt",
            "1",
            "-tile-columns",
            "2",
        ],
        systems: ANY,
        addon_default: true,
    },
];

/// The encoders for `codec` on operating system `os` (`std::env::consts::OS`), in the order they are tried.
pub(crate) fn encoders_for(codec: VideoCodec, os: &str) -> Vec<&'static Encoder> {
    let all = match codec {
        VideoCodec::Vp9 => VP9_ENCODERS,
        _ => H264_ENCODERS,
    };
    all.iter().filter(|e| e.systems.contains(&os)).collect()
}

/// The order to try encoders in, given which ones this ffmpeg has (`listed`, `None` when it could not be asked): every listed hardware encoder, then the listed software ones. When ffmpeg could not be asked, only the encoder the media add-on's build has is tried.
pub(crate) fn encoder_order<'a>(
    candidates: &[&'a Encoder],
    listed: Option<&HashSet<String>>,
) -> Vec<&'a Encoder> {
    match listed {
        Some(listed) => {
            let has = |e: &&&Encoder| listed.contains(e.name);
            let hardware = candidates.iter().filter(|e| e.hardware).filter(has);
            let software = candidates.iter().filter(|e| !e.hardware).filter(has);
            hardware.chain(software).copied().collect()
        }
        None => candidates
            .iter()
            .filter(|e| e.addon_default)
            .copied()
            .collect(),
    }
}

/// The encoder names in `ffmpeg -encoders` output.
pub(crate) fn listed_encoders(output: &str) -> HashSet<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let flags = words.next()?;
            let name = words.next()?;
            // Lines look like ` V....D libx264   libx264 H.264 ...`.
            (flags.len() == 6
                && flags.starts_with(['V', 'A', 'S'])
                && flags
                    .bytes()
                    .skip(1)
                    .all(|b| b == b'.' || b.is_ascii_uppercase())
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
            .then(|| name.to_string())
        })
        .collect()
}

/// The filters before `encoder`: the frame size rounded down to even numbers (which every H.264 and VP9 encoder needs, and which a site's odd-sized video would break otherwise), then the encoder's own.
pub(super) fn encode_filter(encoder: &Encoder) -> String {
    format!("scale=trunc(iw/2)*2:trunc(ih/2)*2,{}", encoder.filter)
}

/// The clip an encoder is tested on: generated, small, a second long.
const PROBE_CLIP: &str = "testsrc2=size=320x240:rate=25:duration=1";
/// Frames of [`PROBE_CLIP`] encoded: enough for an encoder to start and fail if it will.
const PROBE_FRAMES: &str = "10";
/// The bit rate the test asks for, one every encoder takes.
const PROBE_KBPS: &str = "1000k";

/// ffmpeg's arguments to try encoder `encoder` on a short generated clip, which tells whether it works on this computer (a hardware encoder may be listed and still have no device to run on).
pub(crate) fn probe_arguments(encoder: &Encoder) -> Vec<OsString> {
    let mut args = preamble();
    args.extend(encoder.global.iter().map(OsString::from));
    for arg in [
        "-f",
        "lavfi",
        "-i",
        PROBE_CLIP,
        "-frames:v",
        PROBE_FRAMES,
        "-vf",
        &encode_filter(encoder),
        "-c:v",
        encoder.name,
    ] {
        args.push(arg.into());
    }
    args.extend(encoder.options.iter().map(OsString::from));
    for arg in ["-b:v", PROBE_KBPS, "-f", "null", "-"] {
        args.push(arg.into());
    }
    args
}

/// One audio encoder Wirecatch uses for `codec`, given the encoders ffmpeg lists (`None` when it could not be asked): ffmpeg's name and any options it needs.
pub(crate) fn audio_encoder(codec: AudioCodec, listed: Option<&HashSet<String>>) -> AudioEncoder {
    let candidates: &[AudioEncoder] = match codec {
        AudioCodec::Mp3 => &[
            AudioEncoder::plain("libmp3lame"),
            AudioEncoder::plain("mp3_mf"),
        ],
        AudioCodec::Opus => &[
            AudioEncoder::plain("libopus"),
            AudioEncoder {
                name: "opus",
                options: &["-strict", "experimental"],
            },
        ],
        AudioCodec::Flac => &[AudioEncoder::plain("flac")],
        AudioCodec::Pcm => &[AudioEncoder::plain("pcm_s16le")],
        _ => &[AudioEncoder::plain("aac")],
    };
    candidates
        .iter()
        .find(|e| listed.is_none_or(|l| l.contains(e.name)))
        .or(candidates.first())
        .copied()
        .unwrap_or(AudioEncoder::COPY)
}

/// An audio encoder Wirecatch drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AudioEncoder {
    /// ffmpeg's name for it.
    pub name: &'static str,
    /// Options it needs.
    pub options: &'static [&'static str],
}

impl AudioEncoder {
    /// Audio copied as it is.
    pub const COPY: AudioEncoder = AudioEncoder::plain("copy");

    const fn plain(name: &'static str) -> Self {
        Self { name, options: &[] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::output::test_support::*;
    use crate::media::output::*;

    /// Hardware first in a fixed order, then software; libx264 is preferred to `OpenH264` when the build has it; an ffmpeg that cannot be asked gets the add-on's software encoder only.
    #[test]
    fn encoders_fall_back_from_hardware_to_software() {
        let names = |list: Vec<&Encoder>| list.iter().map(|e| e.name).collect::<Vec<_>>();
        let windows = encoders_for(VideoCodec::H264, "windows");
        assert_eq!(
            names(windows.clone()),
            [
                "h264_nvenc",
                "h264_qsv",
                "h264_amf",
                "h264_mf",
                "libx264",
                "libopenh264"
            ]
        );
        let linux = encoders_for(VideoCodec::H264, "linux");
        assert_eq!(
            names(linux.clone()),
            [
                "h264_nvenc",
                "h264_qsv",
                "h264_amf",
                "h264_vaapi",
                "libx264",
                "libopenh264"
            ]
        );
        // The media add-on's LGPL build: no libx264.
        let lgpl: HashSet<String> = [
            "h264_nvenc",
            "h264_qsv",
            "h264_amf",
            "h264_mf",
            "libopenh264",
            "aac",
        ]
        .map(String::from)
        .into();
        assert_eq!(
            names(encoder_order(&windows, Some(&lgpl))),
            [
                "h264_nvenc",
                "h264_qsv",
                "h264_amf",
                "h264_mf",
                "libopenh264"
            ]
        );
        let gpl: HashSet<String> = ["libx264", "libopenh264"].map(String::from).into();
        assert_eq!(
            names(encoder_order(&linux, Some(&gpl))),
            ["libx264", "libopenh264"]
        );
        assert_eq!(names(encoder_order(&linux, None)), ["libopenh264"]);
        let vp9 = encoders_for(VideoCodec::Vp9, "windows");
        assert_eq!(names(encoder_order(&vp9, None)), ["libvpx-vp9"]);
        assert_eq!(
            names(encoders_for(VideoCodec::Vp9, "linux")),
            ["vp9_qsv", "vp9_vaapi", "libvpx-vp9"]
        );
        // Audio encoders fall back too.
        let lame_less: HashSet<String> = ["mp3_mf"].map(String::from).into();
        assert_eq!(
            audio_encoder(AudioCodec::Mp3, Some(&lame_less)).name,
            "mp3_mf"
        );
        assert_eq!(audio_encoder(AudioCodec::Mp3, None).name, "libmp3lame");
        assert_eq!(audio_encoder(AudioCodec::Aac, Some(&lgpl)).name, "aac");
    }

    #[test]
    fn the_encoder_list_is_read_from_ffmpeg() {
        let text = "Encoders:\n V..... = Video\n ------\n V....D libopenh264          OpenH264 H.264\n V..... h264_qsv             H.264 (Intel)\n A....D aac                  AAC\n S..... mov_text             3GPP\n";
        let listed = listed_encoders(text);
        for name in ["libopenh264", "h264_qsv", "aac", "mov_text"] {
            assert!(listed.contains(name), "{name}");
        }
        assert!(!listed.contains("="));
        assert!(!listed.contains("Video"));
    }

    #[test]
    fn probing_encodes_a_generated_clip_to_nowhere() {
        let nvenc = &H264_ENCODERS[0];
        let joined = text(&probe_arguments(nvenc)).join(" ");
        assert!(joined.contains("-f lavfi -i testsrc2="), "{joined}");
        assert!(joined.contains("-c:v h264_nvenc -preset p5"));
        assert!(joined.ends_with("-f null -"));
        let vaapi = H264_ENCODERS
            .iter()
            .find(|e| e.name == "h264_vaapi")
            .unwrap();
        assert!(text(&probe_arguments(vaapi)).join(" ").starts_with(
            "-hide_banner -nostdin -loglevel error -y -vaapi_device /dev/dri/renderD128 -f lavfi"
        ));
    }
}
