use discworld_core::graphics::{Handle, Image, Resources, decode_tiles, read_palette};
use discworld_core::iso::Iso;
use discworld_core::resource::{chunks, parse_index};
use md5::Md5;
use serde_json::json;
use sha2::{Digest, Sha256};
mod audio;
mod music;
mod probe;
mod render;
mod window;

fn main() {
    if let Err(error) = run() {
        eprintln!("discworld-tools: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let command = args
        .next()
        .ok_or("usage: discworld-tools inspect-iso <image.iso>")?;
    let path = args.next().ok_or("missing ISO path")?;
    if command == "preview-conversation" || command == "preview-inventory" {
        return window::preview(path, args);
    }
    if command == "audit-text" {
        if args.next().is_some() {
            return Err("unexpected argument".into());
        }
        let data = std::fs::read(path)?;
        let iso = Iso::parse(&data)?;
        let strings = discworld_core::text::Strings::new(
            iso.file("/DISCWLD/ENGLISH.TXT").ok_or("missing text")?,
        )?;
        let index = parse_index(iso.file("/DISCWLD/INDEX").ok_or("missing index")?)?;
        let resources = Resources(
            index
                .iter()
                .map(|e| {
                    iso.file(&format!("/DISCWLD/{}", e.name))
                        .ok_or("missing resource")
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
        let engine = discworld_core::engine::Engine::bootstrap(&resources)?;
        let mut fonts = Vec::new();
        for handle in [engine.declarations.tag_font, engine.declarations.talk_font]
            .into_iter()
            .flatten()
        {
            let font = discworld_core::text::Font::read(&resources, handle)?;
            let mut glyph_count = 0;
            for h in font.glyphs {
                if h.0 != 0 {
                    let i = Image::read(&resources, h)?;
                    decode_tiles(&resources, &i)?;
                    glyph_count += 1;
                }
            }
            for id in 0..strings.len() {
                font.centered(&resources, strings.get(id as u32)?, [160, 100])?;
            }
            fonts.push(json!({"handle":handle.0,"glyph_count":glyph_count,"flags":font.flags}));
        }
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"strings":strings.len(),"fonts":fonts,"scope":"String bounds, glyph tile decoding, and centered layout; no runtime PRINT or independent pixel comparison"})
            )?
        );
        return Ok(());
    }
    if command == "render-music" {
        return music::render(path, args);
    }
    if command == "audit-music" || command == "sequence-music" {
        if args.next().is_some() {
            return Err("unexpected argument".into());
        }
        let data = std::fs::read(path)?;
        let iso = Iso::parse(&data)?;
        let tracks = discworld_core::music::archive(
            iso.file("/DISCWLD/MIDI.DAT")
                .ok_or("missing MIDI archive")?,
        )?;
        let mut records = Vec::new();
        for track in tracks {
            let events = discworld_core::music::events(track.events)
                .map_err(|e| format!("music offset {}: {e}", track.offset))?;
            let controls = events
                .iter()
                .filter(|e| e.status & 0xf0 == 0xb0)
                .map(|e| e.data[0])
                .collect::<std::collections::BTreeSet<_>>();
            let sequencing = if command == "sequence-music" {
                let mut player = discworld_core::sequencer::Sequencer::new(track.events, false)?;
                let mut messages = 0usize;
                let mut hash = Sha256::new();
                for _ in 0..1440 {
                    for message in player
                        .advance(5)
                        .map_err(|e| format!("music offset {}: {e}", track.offset))?
                    {
                        messages += 1;
                        hash.update(message.tick.to_le_bytes());
                        hash.update([message.status]);
                        hash.update((message.data.len() as u32).to_le_bytes());
                        hash.update(&message.data);
                    }
                }
                Some(
                    json!({"seconds":60,"messages":messages,"still_playing":player.playing,"sha256":format!("{:x}",hash.finalize())}),
                )
            } else {
                None
            };
            records.push(json!({"offset":track.offset,"event_count":events.len(),
                "encoded_ticks":events.iter().map(|e| u64::from(e.delta)).sum::<u64>(),
                "controllers":controls,"sequencing":sequencing,"sha256":format!("{:x}",Sha256::digest(track.events))}));
        }
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"scope":"XMIDI decoding with optional 60-second sequencing trace; no synthesis or independent timing comparison", "tracks":records})
            )?
        );
        return Ok(());
    }
    if command == "audit-polygons" {
        if args.next().is_some() {
            return Err("unexpected argument".into());
        }
        let data = std::fs::read(path)?;
        let iso = Iso::parse(&data)?;
        println!("{}", serde_json::to_string_pretty(&probe::polygons(&iso)?)?);
        return Ok(());
    }
    if command == "extract-assets" {
        let destination =
            std::path::PathBuf::from(args.next().ok_or("missing new destination directory")?);
        if args.next().is_some() {
            return Err("unexpected argument".into());
        }
        let data = std::fs::read(path)?;
        let iso = Iso::parse(&data)?;
        std::fs::create_dir(&destination)?;
        for entry in &iso.entries {
            let Some(name) = entry.path.strip_prefix("/DISCWLD/") else {
                continue;
            };
            if name.contains('/') {
                return Err("nested game path unsupported".into());
            }
            let output = destination.join(name);
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output)?;
            std::io::Write::write_all(&mut file, &data[entry.range.clone()])?;
        }
        eprintln!("Extracted DISCWLD files to {}", destination.display());
        return Ok(());
    }
    if command == "audit-audio" {
        if args.next().is_some() {
            return Err("unexpected argument".into());
        }
        let data = std::fs::read(path)?;
        let iso = Iso::parse(&data)?;
        println!("{}", serde_json::to_string_pretty(&audio::audit(&iso)?)?);
        return Ok(());
    }
    if command == "run-headless" {
        return render::headless(path, args);
    }
    if command == "render-reel" {
        return render::reel(path, args);
    }
    if command == "render-image" {
        let name = args
            .next()
            .ok_or("missing resource name")?
            .into_string()
            .map_err(|_| "invalid resource name")?;
        let ordinal: usize = args
            .next()
            .ok_or("missing image ordinal")?
            .into_string()
            .map_err(|_| "invalid ordinal")?
            .parse()?;
        let destination = args.next().ok_or("missing output PNG path")?;
        if args.next().is_some() {
            return Err("unexpected argument".into());
        }
        let data = std::fs::read(path)?;
        let iso = Iso::parse(&data)?;
        let index = parse_index(iso.file("/DISCWLD/INDEX").ok_or("missing resource index")?)?;
        let id = index
            .iter()
            .position(|e| e.name.eq_ignore_ascii_case(&name))
            .ok_or("unknown resource")?;
        let files = index
            .iter()
            .map(|e| {
                iso.file(&format!("/DISCWLD/{}", e.name))
                    .ok_or("missing indexed file")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let resources = Resources(files);
        let headers = chunks(resources.0[id])?;
        let images = headers
            .iter()
            .find(|c| c.kind == 0x33340006)
            .ok_or("missing image chunk")?;
        let offset = ordinal
            .checked_mul(16)
            .and_then(|n| images.payload.start.checked_add(n))
            .ok_or("image offset overflow")?;
        if offset
            .checked_add(16)
            .is_none_or(|end| end > images.payload.end)
        {
            return Err("image ordinal out of range".into());
        }
        let image = Image::read(&resources, Handle(((id as u32) << 23) | offset as u32))?;
        let decoded = decode_tiles(&resources, &image)?;
        let palette = read_palette(&resources, image.palette)?;
        let rgba = decoded.rgba(&palette)?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        let mut encoder = png::Encoder::new(
            std::io::BufWriter::new(file),
            image.width.into(),
            image.height.into(),
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&rgba)?;
        eprintln!(
            "Rendered {name} image {ordinal}: {}x{}",
            image.width, image.height
        );
        return Ok(());
    }
    if (command != "inspect-iso"
        && command != "inspect-resources"
        && command != "audit-images"
        && command != "probe-scripts"
        && command != "startup")
        || args.next().is_some()
    {
        return Err("usage: discworld-tools <inspect-iso|inspect-resources> <image.iso>".into());
    }
    let data = std::fs::read(path)?;
    let iso = Iso::parse(&data)?;
    if command == "startup" {
        println!("{}", serde_json::to_string_pretty(&probe::startup(&iso)?)?);
        return Ok(());
    }
    if command == "probe-scripts" {
        let report = probe::run(&iso)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        if report["errors"].as_u64() != Some(0) {
            return Err("script probe failures; see JSON report".into());
        }
        return Ok(());
    }
    if command == "audit-images" {
        let index = parse_index(iso.file("/DISCWLD/INDEX").ok_or("missing resource index")?)?;
        let resources = Resources(
            index
                .iter()
                .map(|e| {
                    iso.file(&format!("/DISCWLD/{}", e.name))
                        .ok_or("missing indexed file")
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
        let mut decoded_count = 0usize;
        let mut errors = Vec::new();
        let mut reports = Vec::new();
        for (id, entry) in index.iter().enumerate() {
            let headers = chunks(resources.0[id])?;
            let Some(images) = headers.iter().find(|c| c.kind == 0x33340006) else {
                continue;
            };
            if images.payload.len() % 16 != 0 {
                return Err(format!("unaligned image table {}", entry.name).into());
            }
            let mut hash = Sha256::new();
            let mut successful = 0;
            for offset in (images.payload.start..images.payload.end).step_by(16) {
                let result = (|| {
                    let image =
                        Image::read(&resources, Handle(((id as u32) << 23) | offset as u32))?;
                    let decoded = decode_tiles(&resources, &image)?;
                    hash.update(image.width.to_le_bytes());
                    hash.update(image.height.to_le_bytes());
                    hash.update(&decoded.pixels);
                    hash.update(
                        decoded
                            .coverage
                            .iter()
                            .map(|c| u8::from(*c))
                            .collect::<Vec<_>>(),
                    );
                    Ok::<_, discworld_core::iso::Error>(())
                })();
                match result {
                    Ok(()) => {
                        decoded_count += 1;
                        successful += 1;
                    }
                    Err(e) => errors.push(
                        json!({"resource":entry.name,"image_offset":offset,"error":e.to_string()}),
                    ),
                }
            }
            reports.push(json!({"resource":entry.name,"decoded":successful,"indexed_pixels_and_coverage_sha256":format!("{:x}",hash.finalize())}));
        }
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"schema_version":1,"decoded_images":decoded_count,"failed_images":errors.len(),"errors":errors,"resources":reports,"scope":"Indexed tile decoding only; no palette allocation or reference pixel comparison"})
            )?
        );
        if !errors.is_empty() {
            return Err("image decoding failures; see JSON report".into());
        }
        return Ok(());
    }
    if command == "inspect-resources" {
        let index = parse_index(iso.file("/DISCWLD/INDEX").ok_or("missing resource index")?)?;
        let mut resources = Vec::new();
        for (id, entry) in index.iter().enumerate() {
            let path = format!("/DISCWLD/{}", entry.name);
            let content = iso
                .file(&path)
                .ok_or_else(|| format!("missing resource {path}"))?;
            if content.len() != entry.size as usize {
                return Err(format!("resource size mismatch: {path}").into());
            }
            if entry.flags & 0x10 != 0 {
                return Err(format!("compressed resource unsupported: {path}").into());
            }
            let headers = chunks(content).map_err(|e| format!("{path}: {e}"))?;
            let list: Vec<_> = headers.iter().map(|c| json!({"type": format!("0x{:08x}", c.kind), "header_offset": c.header_offset, "payload_start": c.payload.start, "payload_end": c.payload.end})).collect();
            resources.push(json!({"id":id, "name":entry.name, "bytes":entry.size, "flags":entry.flags, "chunks":list}));
        }
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"schema_version":1,"resource_count":resources.len(),"resources":resources})
            )?
        );
        return Ok(());
    }
    let files: Vec<_> = iso
        .entries
        .iter()
        .map(|e| {
            let content = &data[e.range.clone()];
            json!({"path": e.path, "bytes": content.len(), "offset": e.range.start,
            "sha256": format!("{:x}", Sha256::digest(content)),
            "md5_first_5000": format!("{:x}", Md5::digest(&content[..content.len().min(5000)]))})
        })
        .collect();
    let dw = iso.file("/DISCWLD/DW.SCN");
    let matched = dw.is_some_and(|b| {
        b.len() == 776188
            && format!("{:x}", Md5::digest(&b[..5000])) == "70955425870c7720d6eebed903b2ef41"
    }) && iso.file("/DISCWLD/ENGLISH.SMP").is_some();
    let output = json!({"schema_version": 1, "volume_id": iso.volume_id, "iso_bytes": data.len(),
        "iso_sha256": format!("{:x}", Sha256::digest(&data)), "file_count": files.len(),
        "detected_release": if matched { Some("Discworld English DOS CD v2 (Tinsel V1)") } else { None },
        "files": files});
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
