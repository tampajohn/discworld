use discworld_core::{
    graphics::{Handle, Resources},
    iso::Iso,
    library,
    resource::{chunks, parse_index},
    scene,
    script::{Machine, Step},
};
use serde_json::{Value, json};

pub fn polygons(iso: &Iso<'_>) -> Result<Value, Box<dyn std::error::Error>> {
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
    let mut scenes = Vec::new();
    let mut total = 0;
    let mut node_count = 0;
    let mut script_count = 0;
    for (resource, entry) in index.iter().enumerate() {
        let Some(scene) = scene::read(&resources, resource)? else {
            continue;
        };
        let polygons =
            discworld_core::polygon::read(&resources, scene.polygons, scene.polygon_count)
                .map_err(|e| format!("{}: {e}", entry.name))?;
        let mut records = Vec::new();
        for (ordinal, polygon) in polygons.iter().enumerate() {
            if polygon.script.0 != 0 {
                scene::script_bytes(&resources, polygon.script)
                    .map_err(|e| format!("{} polygon {ordinal}: {e}", entry.name))?;
                script_count += 1;
            }
            node_count += polygon.nodes.len();
            let projections = polygon
                .nodes
                .iter()
                .map(|point| {
                    discworld_core::nodepath::project(polygon, *point)
                        .map(|p| json!({"input":point,"position":p.position,"line":p.line}))
                        .map_err(|e| format!("{} polygon {ordinal}: {e}", entry.name))
                })
                .collect::<Result<Vec<_>, _>>()?;
            records.push(
                json!({"ordinal":ordinal,"kind":format!("{:?}",polygon.kind),
                "id":polygon.id,"corners":polygon.corners,"nodes":polygon.nodes,
                "script":polygon.script.0,"node_projections":projections}),
            );
        }
        total += polygons.len();
        scenes.push(json!({"resource":entry.name,"polygons":records}));
    }
    Ok(
        json!({"scope":"Checked V1 polygon records, node arrays, line coefficients, and script bounds; no pathfinding or hit-test equivalence", "polygon_count":total,"node_count":node_count,"script_count":script_count,"scenes":scenes}),
    )
}

pub fn startup(iso: &Iso<'_>) -> Result<Value, Box<dyn std::error::Error>> {
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
    let master = resources.0[0];
    let headers = chunks(master)?;
    let global_chunk = headers
        .iter()
        .find(|c| c.kind == 0x33340010)
        .ok_or("missing global count")?;
    let count = discworld_core::film::word(&master[global_chunk.payload.clone()], 0)? as usize;
    if count > 65536 {
        return Err("global count limit".into());
    }
    let chunk = headers
        .iter()
        .find(|c| c.kind == 0x3334000a)
        .ok_or("missing master code")?;
    let code = &master[chunk.payload.clone()];
    let mut vm = Machine::default();
    let mut globals = vec![0; count];
    let mut state = discworld_core::startup::Declarations::default();
    let mut calls = Vec::new();
    let mut stop = json!({"status":"instruction_budget"});
    for steps in 1..=10000 {
        match vm.step(code, &mut globals)? {
            Step::Continue | Step::Escape(_) => {}
            Step::Halted => {
                stop = json!({"status":"halted","steps":steps});
                break;
            }
            Step::Library(call) => {
                let before = vm.operands().to_vec();
                let applied = state.apply(&mut vm, call, &resources).map_err(|e| {
                    format!(
                        "{}: {e}; stack={before:?}",
                        library::name(call.id).unwrap_or("unknown")
                    )
                })?;
                calls.push(json!({"id":call.id,"name":library::name(call.id),"ip":call.instruction,"applied":applied,"stack":before}));
                if !applied {
                    stop = json!({"status":"unsupported_engine_operation","id":call.id,"name":library::name(call.id),"steps":steps});
                    break;
                }
            }
        }
    }
    Ok(
        json!({"schema_version":1,"scope":"Master script declaration initialization only; no scene startup or interactive runtime", "stop":stop,"calls":calls,"declarations":{
            "cursor":state.cursor.as_ref().map(|(h,f)|json!({"handle":h.0,"rate":f.rate,"reels":f.reels.iter().map(|r|json!({"object":r.object.0,"script":r.script.0})).collect::<Vec<_>>()})),
            "lead":state.lead.as_ref().map(|l|json!({"id":l.id,"text":l.text.0,"tagged":l.tagged})),
            "movers":state.movers.iter().map(|(id,reels)|json!({"id":id,"reels":reels.iter().map(|scale|scale.iter().map(|h|h.0).collect::<Vec<_>>()).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            "window_parts":state.window_parts.map(|h|h.0),"tag_font":state.tag_font.map(|h|h.0),"talk_font":state.talk_font.map(|h|h.0),
            "windows":state.windows.iter().map(|w|w.as_ref().map(|w|json!({"title":w.title.0,"capacity":w.capacity,"min":w.min,"max":w.max,"size":w.size,"position":w.position,"contents":w.contents}))).collect::<Vec<_>>(),
            "permanent_topics":state.permanent_topics,"speech_colors":state.speech_colors,"scaling_reel_count":state.scaling.len(),
            "config_strings":state.config_strings.map(|a|a.map(|h|h.0))
        },"globals":globals}),
    )
}

/// Probe script prefixes only. Each starts with fresh zero globals and stops at
/// its first host operation. This does not simulate an actual scene startup.
pub fn run(iso: &Iso<'_>) -> Result<Value, Box<dyn std::error::Error>> {
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
    let master = resources.0[0];
    let master_headers = chunks(master)?;
    let global_chunk = master_headers
        .iter()
        .find(|c| c.kind == 0x33340010)
        .ok_or("missing globals count")?;
    let count = u32::from_le_bytes(
        master[global_chunk.payload.start..global_chunk.payload.start + 4].try_into()?,
    );
    if count > 65536 {
        return Err("global count limit".into());
    }
    let code_chunk = master_headers
        .iter()
        .find(|c| c.kind == 0x3334000a)
        .ok_or("missing master code")?;
    let mut roots = vec![(
        "dw.scn".to_owned(),
        "master".to_owned(),
        Handle(code_chunk.payload.start as u32),
    )];
    let mut scenes = Vec::new();
    for (id, entry) in index.iter().enumerate() {
        let Some(s) = scene::read(&resources, id)? else {
            continue;
        };
        if s.script.0 != 0 {
            roots.push((entry.name.clone(), "scene".to_owned(), s.script));
        }
        for (number, handle) in &s.entrances {
            if handle.0 != 0 {
                roots.push((entry.name.clone(), format!("entrance:{number}"), *handle));
            }
        }
        for actor in &s.actors {
            if actor.script.0 != 0 {
                roots.push((
                    entry.name.clone(),
                    format!("actor:{}", actor.id),
                    actor.script,
                ));
            }
        }
        for (ordinal, polygon) in
            discworld_core::polygon::read(&resources, s.polygons, s.polygon_count)?
                .iter()
                .enumerate()
        {
            if polygon.script.0 != 0 {
                roots.push((
                    entry.name.clone(),
                    format!("polygon:{ordinal}:{}", polygon.id),
                    polygon.script,
                ));
            }
        }
        scenes.push(json!({"name":entry.name,"entrances":s.entrances.len(),"actors":s.actors.len(),"polygons":s.polygon_count,"script":s.script.0}));
    }
    let mut results = Vec::new();
    for (resource, kind, handle) in roots {
        let result = (|| -> Result<Value, Box<dyn std::error::Error>> {
            let code = scene::script_bytes(&resources, handle)?;
            let mut vm = Machine::default();
            let mut globals = vec![0; count as usize];
            for steps in 1..=10000 {
                match vm.step(code, &mut globals)? {
                    Step::Continue | Step::Escape(_) => {}
                    Step::Halted => return Ok(json!({"status":"halted","steps":steps})),
                    Step::Library(call) => {
                        return Ok(
                            json!({"status":"library_boundary","steps":steps,"ip":vm.ip(),"library_id":call.id,"library_name":library::name(call.id),"stack":vm.operands()}),
                        );
                    }
                }
            }
            Ok(json!({"status":"instruction_budget"}))
        })();
        results.push(json!({"resource":resource,"kind":kind,"handle":handle.0,"result":match result {Ok(v)=>v,Err(e)=>json!({"status":"error","message":e.to_string()})}}));
    }
    let errors = results
        .iter()
        .filter(|r| r["result"]["status"] == "error")
        .count();
    Ok(
        json!({"schema_version":1,"scope":"Independent script prefixes with fresh zero globals; stops at first library call; not a gameplay trace","global_count":count,"scene_count":scenes.len(),"script_count":results.len(),"errors":errors,"scenes":scenes,"scripts":results}),
    )
}
