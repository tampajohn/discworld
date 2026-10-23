//! Compare independent, per-animation ScummVM opcode prefixes with our runtime.
use discworld_core::{
    animation::{Animation, Tick},
    film::Reel,
    graphics::{Handle, Resources},
    iso::Iso,
    resource::parse_index,
};
use serde_json::json;
use sha2::{Digest, Sha256};

pub fn compare(
    iso_path: std::ffi::OsString,
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    let trace_path = args.next().ok_or("missing normalized reference trace")?;
    if args.next().is_some() {
        return Err("unexpected trace comparison argument".into());
    }
    let trace_bytes = std::fs::read(trace_path)?;
    let records: Vec<serde_json::Value> = serde_json::from_slice(&trace_bytes)?;
    if records.is_empty() || records.len() > 100_000 {
        return Err("invalid trace record count".into());
    }
    let data = std::fs::read(iso_path)?;
    let iso = Iso::parse(&data)?;
    let index = parse_index(iso.file("/DISCWLD/INDEX").ok_or("missing resource index")?)?;
    let mut resources = Resources(
        index
            .iter()
            .map(|entry| {
                iso.file(&format!("/DISCWLD/{}", entry.name))
                    .ok_or("missing resource")
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    if resources.0.len() >= 512 {
        return Err("no resource slot for diagnostic object".into());
    }
    // Reference InitStepAnimScript logs hImg (an image), not hShape (a frame
    // list). Object shape/capacity do not control opcode order. Use a neutral
    // object and explicitly exclude image/state equivalence from this check.
    let object_data = [0u8; 24];
    let object = Handle((resources.0.len() as u32) << 23);
    resources.0.push(&object_data);
    let mut comparisons = Vec::new();
    let mut total = 0usize;
    let mut unobserved = 0usize;
    for (number, record) in records.iter().enumerate() {
        let script = u32::try_from(record["script"].as_u64().ok_or("invalid script handle")?)?;
        let period = u32::try_from(
            record["period"]
                .as_u64()
                .ok_or("invalid animation period")?,
        )?;
        let expected: Vec<(usize, u32)> = serde_json::from_value(record["operations"].clone())?;
        if expected.len() > 1_000_000 {
            return Err("trace operation limit".into());
        }
        let mut animation = Animation::new(
            &resources,
            &Reel {
                object,
                script: Handle(script),
            },
            period,
        )?;
        if expected.is_empty() {
            unobserved += 1;
            continue;
        }
        let budget = expected
            .len()
            .checked_mul(period as usize)
            .and_then(|n| n.checked_add(1))
            .filter(|n| *n <= 10_000_000)
            .ok_or("trace tick budget limit")?;
        let mut matched = 0;
        let mut ticks = 0;
        while matched < expected.len() && ticks < budget {
            let (status, operations) = animation.tick_traced(&resources)?;
            ticks += 1;
            for actual in operations {
                // Capture can stop mid-tick. Only compare the observed prefix.
                if matched == expected.len() {
                    break;
                }
                if actual != expected[matched] {
                    return Err(format!("record {number}, script {script:#x}, operation {matched}: expected {:?}, got {actual:?}", expected[matched]).into());
                }
                matched += 1;
            }
            if status == Tick::Finished && matched < expected.len() {
                return Err(
                    format!("record {number}: runtime ended before reference prefix").into(),
                );
            }
        }
        if matched != expected.len() {
            return Err("trace comparison exhausted tick budget".into());
        }
        total += matched;
        comparisons.push(json!({"record":number,"script":script,"observed_operations":matched,"simulated_ticks":ticks}));
    }
    if total == 0 {
        return Err("reference trace contains no observed operations".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "iso_sha256":format!("{:x}", Sha256::digest(&data)),
            "trace_sha256":format!("{:x}", Sha256::digest(&trace_bytes)),
            "matched_records":comparisons.len(),"unobserved_records":unobserved,
            "matched_operations":total,"comparisons":comparisons,
            "scope":"Per-instance observed opcode prefixes using original scripts. Does not compare frame pixels, object state, timing, scheduler ordering, input replay or audio. Trace provenance must be verified separately."
        }))?
    );
    Ok(())
}
