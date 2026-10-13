//! Reading manifests: representations, inheritance over levels, segment lists and indexes, and how broken or hostile manifests are refused.

#![forbid(unsafe_code)]

use super::*;
use crate::plan::Piece;

fn base() -> Url {
    Url::parse("https://site.example/dash/manifest.mpd").unwrap()
}

/// A representation's pieces, as `plan_dash` gets them for every kind but an indexed file.
fn pieces(segments: &Segments) -> Result<Vec<Piece>, ManifestError> {
    match segments {
        Segments::Template(template) => template.expand(),
        Segments::List(list) => list.expand(),
        Segments::Whole(url) => Ok(vec![Piece::plain(Url::clone(url), None)]),
        Segments::Indexed { .. } => panic!("an indexed file needs its index read first"),
    }
}

fn names(pieces: &[Piece]) -> Vec<String> {
    pieces
        .iter()
        .map(|p| p.url.as_str().trim_start_matches("https://").to_string())
        .collect()
}

#[test]
fn iso_durations() {
    assert_eq!(iso_duration("PT1H2M3.5S"), Some(3723.5));
    assert_eq!(iso_duration("PT0.5S"), Some(0.5));
    assert_eq!(iso_duration("P1DT1S"), Some(86_401.0));
    assert_eq!(iso_duration("P0Y0M0DT10S"), Some(10.0));
    assert_eq!(iso_duration("P1M"), None);
    assert_eq!(iso_duration("PT"), Some(0.0));
    for bad in ["", "1H", "PT1X", "PT1", "PTxS"] {
        assert_eq!(iso_duration(bad), None, "{bad}");
    }
}

const NUMBER_MPD: &str = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT9.6S">
  <BaseURL>media/</BaseURL>
  <Period>
    <AdaptationSet mimeType="video/mp4" codecs="avc1.64001f" frameRate="30000/1001">
      <SegmentTemplate timescale="1000" duration="3200" startNumber="5"
        initialization="$RepresentationID$/init.mp4" media="$RepresentationID$/$Number%03d$.m4s"/>
      <Representation id="v720" bandwidth="2000000" width="1280" height="720"/>
      <Representation id="v360" bandwidth="600000" width="640" height="360">
        <SegmentTemplate startNumber="1"/>
      </Representation>
    </AdaptationSet>
    <AdaptationSet contentType="audio" lang="en">
      <Role schemeIdUri="urn:mpeg:dash:role:2011" value="main"/>
      <AudioChannelConfiguration schemeIdUri="x" value="2"/>
      <Representation id="a1" bandwidth="128000" codecs="mp4a.40.2">
        <BaseURL>https://cdn.example/audio/</BaseURL>
        <SegmentTemplate timescale="48000" media="seg$Number$.m4s" initialization="init.mp4" duration="153600"/>
      </Representation>
    </AdaptationSet>
    <AdaptationSet contentType="image"><Representation id="thumbs" bandwidth="1"/></AdaptationSet>
  </Period>
</MPD>"#;

#[test]
fn number_templates_inherit_and_count_from_the_duration() {
    let mpd = parse(NUMBER_MPD, &base()).unwrap();
    assert!(!mpd.dynamic);
    assert_eq!(mpd.duration, Some(9.6));
    assert_eq!(mpd.reps.len(), 3, "the image set is left out");
    let v720 = &mpd.reps[0];
    assert_eq!(v720.kind, Kind::Video);
    assert_eq!((v720.width, v720.height), (Some(1280), Some(720)));
    assert!((v720.frame_rate.unwrap() - 29.97).abs() < 0.01);
    assert_eq!(v720.codecs.as_deref(), Some("avc1.64001f"));
    // 9.6 s in 3.2 s segments is exactly 3, not 4 from rounding errors.
    assert_eq!(
        names(&pieces(&v720.segments).unwrap()),
        [
            "site.example/dash/media/v720/init.mp4",
            "site.example/dash/media/v720/005.m4s",
            "site.example/dash/media/v720/006.m4s",
            "site.example/dash/media/v720/007.m4s"
        ]
    );
    let v360 = pieces(&mpd.reps[1].segments).unwrap();
    assert_eq!(names(&v360)[1], "site.example/dash/media/v360/001.m4s");

    let audio = &mpd.reps[2];
    assert_eq!(audio.kind, Kind::Audio);
    assert!(audio.main);
    assert_eq!(audio.language.as_deref(), Some("en"));
    assert_eq!(audio.channels.as_deref(), Some("2"));
    let pieces = pieces(&audio.segments).unwrap();
    assert_eq!(names(&pieces)[0], "cdn.example/audio/init.mp4");
    assert_eq!(names(&pieces)[3], "cdn.example/audio/seg3.m4s");
    assert!(pieces[0].init && !pieces[1].init);
}

#[test]
fn timelines_give_times_and_numbers() {
    let text = r#"<MPD type="static" mediaPresentationDuration="PT10S"><Period>
          <AdaptationSet mimeType="audio/mp4">
            <Representation id="a" bandwidth="1">
              <SegmentTemplate timescale="10" presentationTimeOffset="100" media="t$Time$-n$Number$.m4s">
                <SegmentTimeline>
                  <S t="100" d="20" r="1"/>
                  <S d="30"/>
                  <S t="200" d="25" r="-1"/>
                </SegmentTimeline>
              </SegmentTemplate>
            </Representation>
          </AdaptationSet></Period></MPD>"#;
    let mpd = parse(text, &base()).unwrap();
    let first = pieces(&mpd.reps[0].segments).unwrap();
    // Period end is 100 + 10 s * 10 = 200 ticks... the last entry starts at 200, so r=-1 repeats until the period end: none past it.
    assert_eq!(
        names(&first),
        [
            "site.example/dash/t100-n1.m4s",
            "site.example/dash/t120-n2.m4s",
            "site.example/dash/t140-n3.m4s",
        ]
    );
    let text = text.replace("PT10S", "PT15S");
    let mpd = parse(&text, &base()).unwrap();
    let pieces = pieces(&mpd.reps[0].segments).unwrap();
    // Now the period ends at 250: two segments of 25 ticks from 200.
    assert_eq!(
        names(&pieces)[3..],
        [
            "site.example/dash/t200-n4.m4s",
            "site.example/dash/t225-n5.m4s"
        ]
    );
}

#[test]
fn segment_lists_and_bases() {
    let text = r#"<MPD type="static" mediaPresentationDuration="PT4S"><Period>
          <AdaptationSet mimeType="video/mp4">
            <Representation id="list" bandwidth="1">
              <BaseURL>file.mp4</BaseURL>
              <SegmentList>
                <Initialization range="0-99"/>
                <SegmentURL mediaRange="100-199"/>
                <SegmentURL media="other.mp4" mediaRange="0-9"/>
              </SegmentList>
            </Representation>
            <Representation id="base" bandwidth="2">
              <BaseURL>indexed.mp4</BaseURL>
              <SegmentBase indexRange="700-799"><Initialization range="0-699"/></SegmentBase>
            </Representation>
            <Representation id="whole" bandwidth="3"><BaseURL>whole.webm</BaseURL></Representation>
          </AdaptationSet></Period></MPD>"#;
    let mpd = parse(text, &base()).unwrap();
    let list = pieces(&mpd.reps[0].segments).unwrap();
    assert_eq!(
        list.iter().map(|p| p.range.clone()).collect::<Vec<_>>(),
        [Some(0..100), Some(100..200), Some(0..10)]
    );
    assert_eq!(names(&list)[2], "site.example/dash/other.mp4");
    match &mpd.reps[1].segments {
        Segments::Indexed { url, index } => {
            assert_eq!(url.as_str(), "https://site.example/dash/indexed.mp4");
            assert_eq!(*index, 700..800);
        }
        other => panic!("{other:?}"),
    }
    let whole = pieces(&mpd.reps[2].segments).unwrap();
    assert_eq!(names(&whole), ["site.example/dash/whole.webm"]);
    assert_eq!(whole[0].range, None);
}

/// A version 0 `sidx` box with `first_offset` 10 and a reference for each of `sizes`.
fn sidx_box(sizes: &[u32]) -> Vec<u8> {
    let mut sidx = Vec::new();
    let body_len = 4 + 4 + 4 + 4 + 4 + 2 + 2 + 12 * u32::try_from(sizes.len()).unwrap();
    sidx.extend_from_slice(&(8 + body_len).to_be_bytes());
    sidx.extend_from_slice(b"sidx");
    sidx.extend_from_slice(&[0, 0, 0, 0]); // version 0, flags
    sidx.extend_from_slice(&1u32.to_be_bytes()); // reference_ID
    sidx.extend_from_slice(&1000u32.to_be_bytes()); // timescale
    sidx.extend_from_slice(&0u32.to_be_bytes()); // earliest presentation time
    sidx.extend_from_slice(&10u32.to_be_bytes()); // first_offset
    sidx.extend_from_slice(&[0, 0]); // reserved
    sidx.extend_from_slice(&u16::try_from(sizes.len()).unwrap().to_be_bytes());
    for size in sizes {
        sidx.extend_from_slice(&size.to_be_bytes());
        sidx.extend_from_slice(&[0; 8]);
    }
    sidx
}

#[test]
fn sidx_boxes_give_subsegment_ranges() {
    let sidx = sidx_box(&[500, 300]);
    let url = Url::parse("https://cdn.example/v.mp4").unwrap();
    let index = 700..700 + sidx.len() as u64;
    let pieces = sidx_pieces(&url, &index, &sidx).unwrap();
    let media_start = 700 + sidx.len() as u64 + 10;
    assert_eq!(
        pieces.iter().map(|p| p.range.clone()).collect::<Vec<_>>(),
        [
            Some(0..media_start),
            Some(media_start..media_start + 500),
            Some(media_start + 500..media_start + 800)
        ]
    );
    assert!(pieces[0].init);
    // Truncated or foreign data is refused, never a panic.
    assert!(sidx_pieces(&url, &index, &sidx[..30]).is_none());
    // An empty subsegment would be an empty range: the file is fetched whole instead.
    let mut empty = sidx.clone();
    let first_size = empty.len() - 24;
    empty[first_size..first_size + 4].copy_from_slice(&0u32.to_be_bytes());
    assert!(sidx_pieces(&url, &index, &empty).is_none());
    assert!(sidx_pieces(&url, &index, b"0000moof").is_none());
    assert!(sidx_pieces(&url, &index, &[]).is_none());
}

/// A box that claims to be smaller than its own header would start the media inside the index, or at 0: an empty init section. The file is fetched whole instead.
#[test]
fn a_sidx_box_smaller_than_its_header_is_refused() {
    let url = Url::parse("https://cdn.example/v.mp4").unwrap();
    for size in [1u32, 7] {
        let mut sidx = sidx_box(&[500]);
        sidx[..4].copy_from_slice(&size.to_be_bytes());
        let index = 0..u64::try_from(sidx.len()).unwrap();
        assert!(sidx_pieces(&url, &index, &sidx).is_none(), "{size}");
    }
}

/// Every subsegment repeats the file's address: a long address and the most references would hold half a gigabyte of addresses. The file is fetched whole instead.
#[test]
fn a_sidx_box_with_too_much_address_text_is_refused() {
    let long = "a".repeat(crate::plan::MAX_URL_LEN - 100);
    let url = Url::parse(&format!("https://cdn.example/{long}.mp4")).unwrap();
    let sidx = sidx_box(&vec![1; usize::from(u16::MAX)]);
    let index = 0..u64::try_from(sidx.len()).unwrap();
    assert!(sidx_pieces(&url, &index, &sidx).is_none());
}

/// Each segment's address is the whole base address: an 8 KB `BaseURL` and a long count would otherwise hold gigabytes of addresses from a small manifest.
#[test]
fn a_track_whose_addresses_are_too_long_together_is_refused() {
    let long = "a".repeat(crate::plan::MAX_URL_LEN - 100);
    let text = format!(
        r#"<MPD><Period><AdaptationSet mimeType="video/mp4"><Representation id="v"><BaseURL>{long}/</BaseURL><SegmentTemplate media="$Number$.m4s" duration="1" endNumber="20000"/></Representation></AdaptationSet></Period></MPD>"#
    );
    let mpd = parse(&text, &base()).unwrap();
    // Not `unwrap_err`, which would print every address on failure.
    let error = pieces(&mpd.reps[0].segments).err();
    assert_eq!(error, Some(ManifestError::AddressesTooLong));
}

#[test]
fn live_drm_and_periods_are_reported() {
    let text = r#"<MPD type="dynamic"><Period id="1"><AdaptationSet mimeType="video/mp4">
            <ContentProtection schemeIdUri="urn:mpeg:dash:mp4protection:2011" value="cenc"/>
            <ContentProtection schemeIdUri="urn:uuid:EDEF8BA9-79D6-4ACE-A3C8-27DCD51D21ED"/>
            <Representation id="v" bandwidth="1"><SegmentTemplate media="$Number$.m4s" duration="1"/></Representation>
          </AdaptationSet></Period><Period id="2"/></MPD>"#;
    let mpd = parse(text, &base()).unwrap();
    assert!(mpd.dynamic);
    assert_eq!(mpd.periods, 2);
    assert_eq!(mpd.reps[0].drm, Some(DrmSystem::Widevine));
    // Without a duration, a duration-based template cannot be counted.
    assert!(pieces(&mpd.reps[0].segments).is_err());
}

#[test]
fn numbers_that_overflow_are_errors() {
    let max = u64::MAX;
    let mpd = |inner: &str| {
        format!(
            r#"<MPD mediaPresentationDuration="PT10S"><Period><AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="1">{inner}</Representation></AdaptationSet></Period></MPD>"#
        )
    };
    // startNumber + n, times past the largest number, and `last + 1`.
    for inner in [
        format!(r#"<SegmentTemplate media="$Number$" duration="1" startNumber="{max}"/>"#),
        format!(r#"<SegmentTemplate media="$Time$" duration="1" presentationTimeOffset="{max}"/>"#),
        format!(
            r#"<SegmentTemplate media="$Time$"><SegmentTimeline><S t="{}" d="5" r="3"/></SegmentTimeline></SegmentTemplate>"#,
            max - 6
        ),
    ] {
        let parsed = parse(&mpd(&inner), &base()).unwrap();
        let error = pieces(&parsed.reps[0].segments).unwrap_err();
        assert_eq!(error, ManifestError::NumberTooLarge);
    }
    let inner = format!(r#"<SegmentList><SegmentURL mediaRange="0-{max}"/></SegmentList>"#);
    assert_eq!(
        parse(&mpd(&inner), &base()).unwrap_err().cause,
        ManifestError::BadByteRange(format!("0-{max}"))
    );
}

#[test]
fn periods_too_long_to_count_have_too_many_segments() {
    let huge = format!("PT{}S", "9".repeat(40));
    for segments in [
        r#"<SegmentTemplate media="$Number$" timescale="1000" duration="1"/>"#,
        r#"<SegmentTemplate media="$Time$"><SegmentTimeline><S d="1" r="-1"/></SegmentTimeline></SegmentTemplate>"#,
    ] {
        let text = format!(
            r#"<MPD mediaPresentationDuration="{huge}"><Period><AdaptationSet mimeType="video/mp4"><Representation id="v">{segments}</Representation></AdaptationSet></Period></MPD>"#
        );
        let mpd = parse(&text, &base()).unwrap();
        let error = pieces(&mpd.reps[0].segments).unwrap_err();
        assert_eq!(error, ManifestError::TooManySegments, "{segments}");
    }
}

#[test]
fn a_track_holds_at_most_the_segment_limit_init_section_included() {
    let max = crate::plan::MAX_SEGMENTS;
    let mpd = |last: u64| {
        format!(
            r#"<MPD><Period><AdaptationSet mimeType="video/mp4"><Representation id="v"><SegmentTemplate media="$Number$.m4s" initialization="init.mp4" duration="1" endNumber="{last}"/></Representation></AdaptationSet></Period></MPD>"#
        )
    };
    let fits = parse(&mpd(max - 1), &base()).unwrap();
    assert_eq!(
        pieces(&fits.reps[0].segments).unwrap().len(),
        usize::try_from(max).unwrap()
    );
    let over = parse(&mpd(max), &base()).unwrap();
    assert_eq!(
        pieces(&over.reps[0].segments).map(|p| p.len()),
        Err(ManifestError::TooManySegments)
    );

    let list = |segments: u64| {
        format!(
            r#"<MPD><Period><AdaptationSet mimeType="video/mp4"><Representation id="v"><SegmentList><Initialization/>{}</SegmentList></Representation></AdaptationSet></Period></MPD>"#,
            "<SegmentURL/>".repeat(usize::try_from(segments).unwrap())
        )
    };
    let fits = parse(&list(max - 1), &base()).unwrap();
    assert_eq!(
        pieces(&fits.reps[0].segments).map(|p| p.len()),
        Ok(usize::try_from(max).unwrap())
    );
    let over = parse(&list(max), &base()).unwrap();
    assert_eq!(
        pieces(&over.reps[0].segments).map(|p| p.len()),
        Err(ManifestError::TooManySegments)
    );
}

/// Numbers a manifest cannot mean, and addresses that are not web addresses, are refused rather than read as something else.
#[test]
fn impossible_numbers_and_addresses_are_refused() {
    let rep = |inner: &str| {
        format!(
            r#"<MPD mediaPresentationDuration="PT10S"><Period><AdaptationSet mimeType="video/mp4"><Representation id="v">{inner}</Representation></AdaptationSet></Period></MPD>"#
        )
    };
    for (text, expected) in [
        (
            rep(
                r#"<SegmentTemplate media="$Time$"><SegmentTimeline><S d="x"/></SegmentTimeline></SegmentTemplate>"#,
            ),
            ManifestError::BadNumber {
                name: "d",
                value: "x".into(),
            },
        ),
        (
            rep(
                r#"<SegmentTemplate media="$Time$"><SegmentTimeline><S t="-5" d="1"/></SegmentTimeline></SegmentTemplate>"#,
            ),
            ManifestError::BadNumber {
                name: "t",
                value: "-5".into(),
            },
        ),
        (
            rep(
                r#"<SegmentTemplate media="$Time$"><SegmentTimeline><S d="1" r="-2"/></SegmentTimeline></SegmentTemplate>"#,
            ),
            ManifestError::BadNumber {
                name: "r",
                value: "-2".into(),
            },
        ),
        (
            rep(r#"<SegmentTemplate media="$Number$" duration="1" startNumber="one"/>"#),
            ManifestError::BadNumber {
                name: "startNumber",
                value: "one".into(),
            },
        ),
        (
            rep("<BaseURL>file:///etc/passwd</BaseURL>"),
            ManifestError::NotWebAddress("file".into()),
        ),
    ] {
        assert_eq!(parse(&text, &base()).unwrap_err().cause, expected, "{text}");
    }
    // Refused when the track is expanded, as a template's addresses are.
    for (inner, expected) in [
        (
            r#"<SegmentList><SegmentURL media="data:,x"/></SegmentList>"#,
            ManifestError::NotWebAddress("data".into()),
        ),
        (
            r#"<SegmentTemplate media="$Number$" duration="1" timescale="0"/>"#,
            ManifestError::ZeroTimescale,
        ),
        (
            r#"<SegmentTemplate media="$Time$"><SegmentTimeline><S t="10" d="5"/><S t="12" d="5"/></SegmentTimeline></SegmentTemplate>"#,
            ManifestError::TimeGoesBack,
        ),
    ] {
        let mpd = parse(&rep(inner), &base()).unwrap();
        let error = pieces(&mpd.reps[0].segments).unwrap_err();
        assert_eq!(error, expected, "{inner}");
    }
}

#[test]
fn hostile_manifests_name_what_is_wrong_and_never_panic() {
    let rep = |inner: &str| {
        format!(
            r#"<MPD mediaPresentationDuration="PT10S"><Period><AdaptationSet mimeType="video/mp4"><Representation id="v">{inner}</Representation></AdaptationSet></Period></MPD>"#
        )
    };
    let huge = "9".repeat(40);
    for (text, expected) in [
        ("<html/>".to_string(), ManifestError::NotMpd),
        ("<MPD/>".into(), ManifestError::NoPeriods),
        (
            rep(r#"<SegmentList><SegmentURL mediaRange="9-1"/></SegmentList>"#),
            ManifestError::BadByteRange("9-1".into()),
        ),
        (
            rep(&format!(
                r#"<SegmentList><SegmentURL mediaRange="0-{huge}"/></SegmentList>"#
            )),
            ManifestError::BadByteRange(format!("0-{huge}")),
        ),
    ] {
        assert_eq!(parse(&text, &base()).unwrap_err().cause, expected, "{text}");
    }
    // Text that is not XML says on which line.
    let error = parse("<MPD>\n<Period/>\n<Set a=1/></MPD>", &base()).unwrap_err();
    assert!(matches!(error.cause, ManifestError::NotXml(_)), "{error:?}");
    assert_eq!(error.line, Some(3));
    assert!(matches!(
        parse("", &base()).unwrap_err().cause,
        ManifestError::NotXml(_)
    ));
    for (inner, expected) in [
            (
                r#"<SegmentTemplate media="$Number%0999d$" duration="1"/>"#.to_string(),
                ManifestError::BadWidth("Number%0999d".into()),
            ),
            (
                r#"<SegmentTemplate media="$Time$"><SegmentTimeline><S d="0"/></SegmentTimeline></SegmentTemplate>"#.to_string(),
                ManifestError::TimelineEntryWithoutDuration,
            ),
            (
                r#"<SegmentTemplate duration="1"/>"#.to_string(),
                ManifestError::TemplateWithoutMedia,
            ),
            (
                r#"<SegmentTemplate media="$Number$"/>"#.to_string(),
                ManifestError::TemplateWithoutDuration,
            ),
            (
                r#"<SegmentTemplate media="$Time$"><SegmentTimeline><S d="1" r="999999999"/></SegmentTimeline></SegmentTemplate>"#.to_string(),
                ManifestError::TooManySegments,
            ),
        ] {
            let mpd = parse(&rep(&inner), &base()).unwrap();
            let error = pieces(&mpd.reps[0].segments).unwrap_err();
            assert_eq!(error, expected, "{inner}");
        }
    // A length too long to be real is no length, and gives no panic.
    let mpd = parse(
        r#"<MPD mediaPresentationDuration="PT99999999999999999999S"><Period/></MPD>"#,
        &base(),
    )
    .unwrap();
    assert!(mpd.duration.is_some_and(f64::is_finite));
}

/// A list on an outer level is kept once for all its representations (`manifest_memory.rs` measures that) and expanded for the chosen ones only, each against the track's own limits: an 8 KB base address repeated by every segment is refused there.
#[test]
fn a_shared_segment_list_is_bounded_when_a_track_is_expanded() {
    let long = "a".repeat(crate::plan::MAX_URL_LEN - 100);
    let segments = crate::plan::MAX_TRACK_URL_BYTES / u64::try_from(long.len()).unwrap() + 1;
    let text = format!(
        r#"<MPD><BaseURL>/{long}/</BaseURL><Period><AdaptationSet mimeType="video/mp4"><SegmentList>{}</SegmentList>{}</AdaptationSet></Period></MPD>"#,
        r#"<SegmentURL media="a"/>"#.repeat(usize::try_from(segments).unwrap()),
        r#"<Representation bandwidth="1"/>"#.repeat(2),
    );
    let mpd = parse(&text, &base()).unwrap();
    assert_eq!(mpd.reps.len(), 2);
    // Not `unwrap_err`, which would print every address on failure.
    let error = pieces(&mpd.reps[0].segments).err();
    assert_eq!(error, Some(ManifestError::AddressesTooLong));
}

/// Every representation looked up its adaptation set's and its period's elements again, each lookup walking past every other representation, so reading took time quadratic in their number (20,000 took two minutes). The nodes looked at are counted, not timed.
#[test]
fn reading_a_manifest_looks_at_each_node_a_bounded_number_of_times() {
    let text = format!(
        r#"<MPD><Period><AdaptationSet mimeType="audio/mp4">{}</AdaptationSet></Period></MPD>"#,
        r#"<Representation bandwidth="1"/>"#.repeat(2000),
    );
    let nodes = u64::try_from(Document::parse(&text).unwrap().descendants().count()).unwrap();
    LOOKED_AT.with(|n| n.set(0));
    let mpd = parse(&text, &base()).unwrap();
    let looked = LOOKED_AT.with(std::cell::Cell::get);
    assert_eq!(mpd.reps.len(), 2000);
    assert!(
        looked <= 32 * nodes,
        "{looked} nodes looked at to read a manifest of {nodes}"
    );
}

#[test]
fn a_manifest_offering_more_representations_than_any_video_is_refused() {
    let max = crate::plan::MAX_REPRESENTATIONS;
    let manifest = |reps: usize| {
        let set = |reps: usize| {
            format!(
                r#"<AdaptationSet mimeType="video/mp4">{}</AdaptationSet>"#,
                r#"<Representation bandwidth="1"/>"#.repeat(reps)
            )
        };
        format!(
            "<MPD><Period>{}{}</Period></MPD>",
            set(max / 2),
            set(reps - max / 2)
        )
    };
    let read = parse(&manifest(max), &base()).map(|mpd| mpd.reps.len());
    assert_eq!(read, Ok(max));
    let error = parse(&manifest(max + 1), &base()).err().map(|e| e.cause);
    assert_eq!(error, Some(ManifestError::TooManyRepresentations));
}
