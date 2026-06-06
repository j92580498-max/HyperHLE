/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Quick-and-dirty decoding of miscellaneous formats (MP3, AAC, CAF) to linear
//! PCM.
//!
//! This should be the only module in touchHLE that makes use of [symphonia].
//!
//! Note on the Symphonia 0.6 API (which differs substantially from 0.5):
//!
//! - [`symphonia::default::get_probe`]'s `probe()` returns the
//!   [`symphonia::core::formats::FormatReader`] directly (boxed), not a
//!   `ProbeResult` with a `format` field.
//! - [`symphonia::core::formats::probe::Hint`] is the probe hint type.
//! - [`symphonia::core::formats::Track::codec_params`] is
//!   `Option<CodecParameters>`, where [`CodecParameters`] is an enum whose
//!   `Audio` variant carries [`AudioCodecParameters`].
//! - Decoders are created with
//!   [`symphonia::core::codecs::registry::CodecRegistry::make_audio_decoder`],
//!   taking `&AudioCodecParameters` and [`AudioDecoderOptions`].
//! - `FormatReader::next_packet()` returns `Result<Option<Packet>>`; `Ok(None)`
//!   signals the end of the stream.
//! - Decoded audio is a [`GenericAudioBufferRef`], which exposes
//!   `copy_bytes_to_vec_interleaved_as::<i16>()` to produce interleaved
//!   little-endian 16-bit PCM bytes directly.
//! - The signal specification type is [`AudioSpec`] (with `rate()` and
//!   `channels()` accessors), replacing 0.5's `SignalSpec`.

use std::io::Cursor;
use symphonia::core::codecs::audio::{AudioDecoderOptions, CODEC_ID_NULL_AUDIO};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// PCM data decoded from a miscellaneous format file.
pub struct SymphoniaDecodedToPcm {
    /// 16-bit little-endian PCM samples, grouped in frames (one sample per
    /// channel in each frame).
    pub bytes: Vec<u8>,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u32,
}

pub fn decode_symphonia_to_pcm(file: Cursor<Vec<u8>>) -> Result<SymphoniaDecodedToPcm, ()> {
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let hint = Hint::new();
    let fmt_opts = FormatOptions::default();
    let meta_opts = MetadataOptions::default();

    // In Symphonia 0.6, `probe()` returns the boxed `FormatReader` directly.
    let mut format = match symphonia::default::get_probe().probe(&hint, mss, fmt_opts, meta_opts) {
        Ok(reader) => reader,
        Err(e) => {
            log!("Symphonia probe failed: {:?}", e);
            return Err(());
        }
    };

    // Find the first audio track with a known codec. `codec_params` is an
    // `Option<CodecParameters>`; we want the `Audio` variant with a non-null
    // codec ID.
    let track = match format.tracks().iter().find(|t| {
        matches!(
            &t.codec_params,
            Some(CodecParameters::Audio(a)) if a.codec != CODEC_ID_NULL_AUDIO
        )
    }) {
        Some(t) => t,
        None => {
            log!("Symphonia: no supported audio tracks found in file");
            return Err(());
        }
    };

    let track_id = track.id;
    // Clone the audio codec parameters so we don't keep a borrow on `format`.
    let audio_params = match &track.codec_params {
        Some(CodecParameters::Audio(a)) => a.clone(),
        // Unreachable: the `find` above already guaranteed the audio variant.
        _ => {
            log!("Symphonia: selected track is not an audio track");
            return Err(());
        }
    };

    // Create the decoder via `get_codecs().make_audio_decoder(...)`.
    let dec_opts = AudioDecoderOptions::default();
    let mut decoder = match symphonia::default::get_codecs()
        .make_audio_decoder(&audio_params, &dec_opts)
    {
        Ok(d) => d,
        Err(e) => {
            log!("Symphonia failed to create decoder: {:?}", e);
            return Err(());
        }
    };

    let mut out_pcm = Vec::<u8>::new();
    // Reused scratch buffer for the interleaved bytes of each decoded packet.
    let mut packet_bytes = Vec::<u8>::new();
    let mut out_rate: Option<u32> = None;
    let mut out_channels: Option<usize> = None;

    loop {
        // `next_packet()` returns `Ok(None)` at the end of the stream. An
        // `IoError` is also treated as a normal end-of-stream.
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::IoError(_)) => break,
            // Chained OGG and similar formats — just stop here.
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => {
                log!("Symphonia packet read error: {:?} (stopping decode)", e);
                break;
            }
        };

        if packet.track_id != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(SymphoniaError::DecodeError(e)) => {
                // Corrupt frame — skip it and keep going.
                log!("Symphonia decode error (recoverable): {:?}", e);
                continue;
            }
            Err(e) => {
                log!("Symphonia fatal decode error: {:?}", e);
                break;
            }
        };

        // Remember the signal spec from the first successfully decoded packet.
        let spec = decoded.spec();
        if out_rate.is_none() {
            out_rate = Some(spec.rate());
            out_channels = Some(spec.channels().count());
        }

        // Convert this packet's samples to interleaved little-endian i16 bytes
        // and append them to the output. `copy_bytes_to_vec_interleaved_as`
        // resizes its destination, so we use a scratch buffer per packet.
        decoded.copy_bytes_to_vec_interleaved_as::<i16>(&mut packet_bytes);
        out_pcm.extend_from_slice(&packet_bytes);
    }

    let (sample_rate, channels) = match (out_rate, out_channels) {
        (Some(rate), Some(channels)) => (rate, channels),
        _ => {
            log!("Symphonia: file yielded no valid audio data");
            return Err(());
        }
    };

    if out_pcm.is_empty() {
        log!("Symphonia: decoded PCM buffer is empty");
        return Err(());
    }

    Ok(SymphoniaDecodedToPcm {
        bytes: out_pcm,
        sample_rate,
        channels: channels.try_into().unwrap(),
    })
}
