use discworld_core::{
    audio::{SAMPLE_RATE, SampleBank},
    iso::Iso,
};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path};
pub fn write_wav(path: &Path, pcm: &[i16]) -> Result<(), Box<dyn std::error::Error>> {
    let size = u32::try_from(pcm.len().checked_mul(2).ok_or("WAV size overflow")?)?;
    let mut file = std::io::BufWriter::new(
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?,
    );
    file.write_all(b"RIFF")?;
    file.write_all(
        &size
            .checked_add(36)
            .ok_or("WAV size overflow")?
            .to_le_bytes(),
    )?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&SAMPLE_RATE.to_le_bytes())?;
    file.write_all(&(SAMPLE_RATE * 2).to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&size.to_le_bytes())?;
    for sample in pcm {
        file.write_all(&sample.to_le_bytes())?;
    }
    file.flush()?;
    Ok(())
}
pub fn audit(iso: &Iso<'_>) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let bank = SampleBank::new(
        iso.file("/DISCWLD/ENGLISH.IDX")
            .ok_or("missing sample index")?,
        iso.file("/DISCWLD/ENGLISH.SMP")
            .ok_or("missing sample data")?,
    )?;
    let mut samples = Vec::new();
    let mut total = 0usize;
    for id in 1..bank.count() {
        if let Some(pcm) = bank.sample(id as i32)? {
            total += pcm.len();
            samples.push(serde_json::json!({"id":id,"samples":pcm.len(),"sha256":format!("{:x}",Sha256::digest(pcm))}));
        }
    }
    Ok(
        serde_json::json!({"schema_version":1,"index_entries":bank.count(),"present_samples":samples.len(),"total_pcm_samples":total,"sample_rate":SAMPLE_RATE,"format":"unsigned 8-bit mono PCM","samples":samples}),
    )
}
