use discworld_core::{iso::Iso, music, sequencer::Sequencer};
pub fn render(
    path: std::ffi::OsString,
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    let offset: u32 = args
        .next()
        .ok_or("missing MIDI offset")?
        .into_string()
        .map_err(|_| "invalid offset")?
        .parse()?;
    let seconds: u32 = args
        .next()
        .ok_or("missing duration")?
        .into_string()
        .map_err(|_| "invalid duration")?
        .parse()?;
    let soundfont = args.next().ok_or("missing SoundFont path")?;
    let output = args.next().ok_or("missing WAV path")?;
    if args.next().is_some() || seconds == 0 || seconds > 600 {
        return Err("invalid arguments or duration".into());
    }
    let data = std::fs::read(path)?;
    let iso = Iso::parse(&data)?;
    let tracks = music::archive(iso.file("/DISCWLD/MIDI.DAT").ok_or("missing MIDI.DAT")?)?;
    let track = tracks
        .iter()
        .find(|t| t.offset == offset)
        .ok_or("unknown track offset")?;
    let mut sequence = Sequencer::new(track.events, false)?;
    let mut synth = discworld_audio::MusicOutput::new(&std::fs::read(soundfont)?, 22050)?;
    let mut pcm = Vec::new();
    let mut message_count = 0;
    for _ in 0..seconds * 24 {
        let messages = sequence.advance(5)?;
        message_count += messages.len();
        let stereo = synth.render(sequence.tick, &messages)?;
        for pair in stereo.chunks_exact(2) {
            pcm.push(
                ((pair[0] + pair[1]) * 0.5 * 32767.0)
                    .round()
                    .clamp(-32768.0, 32767.0) as i16,
            );
        }
    }
    crate::audio::write_wav(std::path::Path::new(&output), &pcm)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"offset":offset,"seconds":seconds,"messages":message_count,"samples":pcm.len(),"nonzero_samples":pcm.iter().filter(|s| **s != 0).count(),"peak":pcm.iter().map(|s| i32::from(*s).abs()).max(),"scope":"XMIDI with TimGM/user SoundFont synthesis; not original-hardware or ScummVM audio equivalence"})
        )?
    );
    Ok(())
}
