// SPDX-License-Identifier: GPL-3.0-or-later
#[derive(Debug, PartialEq, Eq)]
pub enum InteractionEvent {
    Key,
    Move([i32; 2]),
    Scroll(discworld_core::inventory::Scroll),
    World([i32; 2], discworld_core::engine::PolygonInteraction),
    Polygon(discworld_core::engine::PolygonInteraction),
    Actor(discworld_core::engine::PolygonInteraction),
    Inventory(discworld_core::engine::PolygonInteraction),
    Topic,
    CloseConversation,
    CloseInventory,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Interaction {
    pub tick: u32,
    pub scene: String,
    pub target: usize,
    pub event: InteractionEvent,
}
pub fn interactions(
    bytes: &[u8],
    ticks: u32,
) -> Result<Vec<Interaction>, Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let entries = value
        .as_array()
        .ok_or("interaction trace must be an array")?;
    let mut result: Vec<Interaction> = Vec::new();
    for entry in entries {
        let object = entry.as_object().ok_or("interaction must be an object")?;
        let target_key = match entry["event"].as_str() {
            Some("topic") => Some("topic"),
            Some("move") => Some("point"),
            Some(
                "close_conversation" | "close_inventory" | "key" | "scroll_up" | "scroll_down"
                | "page_up" | "page_down" | "inventory_home" | "inventory_end",
            ) => None,
            _ if object.contains_key("point") => Some("point"),
            _ if object.contains_key("inventory_slot") => Some("inventory_slot"),
            _ if object.contains_key("actor") => Some("actor"),
            _ => Some("polygon"),
        };
        let mut keys = vec!["tick", "scene", "event"];
        keys.extend(target_key);
        if object.len() != keys.len() || !keys.iter().all(|key| object.contains_key(*key)) {
            return Err("invalid interaction fields for event".into());
        }
        let tick = u32::try_from(entry["tick"].as_u64().ok_or("invalid interaction tick")?)?;
        if tick >= ticks || result.last().is_some_and(|previous| previous.tick >= tick) {
            return Err("interaction ticks must increase strictly and precede tick limit".into());
        }
        let scene = entry["scene"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("invalid interaction scene")?
            .to_owned();
        let target = target_key
            .filter(|key| *key != "point")
            .map(|key| entry[key].as_u64().ok_or("invalid target index"))
            .transpose()?
            .unwrap_or(0);
        let target = usize::try_from(target)?;
        use discworld_core::engine::PolygonInteraction;
        let event = match entry["event"].as_str() {
            Some("move") => {
                let point = entry["point"]
                    .as_array()
                    .filter(|p| p.len() == 2)
                    .ok_or("invalid cursor point")?;
                let x = i32::try_from(point[0].as_i64().ok_or("invalid cursor x")?)?;
                let y = i32::try_from(point[1].as_i64().ok_or("invalid cursor y")?)?;
                if !(0..320).contains(&x) || !(0..200).contains(&y) {
                    return Err("cursor point outside viewport".into());
                }
                InteractionEvent::Move([x, y])
            }
            Some("key") => InteractionEvent::Key,
            Some("scroll_up") => InteractionEvent::Scroll(discworld_core::inventory::Scroll::Up),
            Some("scroll_down") => {
                InteractionEvent::Scroll(discworld_core::inventory::Scroll::Down)
            }
            Some("page_up") => InteractionEvent::Scroll(discworld_core::inventory::Scroll::PageUp),
            Some("page_down") => {
                InteractionEvent::Scroll(discworld_core::inventory::Scroll::PageDown)
            }
            Some("inventory_home") => {
                InteractionEvent::Scroll(discworld_core::inventory::Scroll::Home)
            }
            Some("inventory_end") => {
                InteractionEvent::Scroll(discworld_core::inventory::Scroll::End)
            }
            Some("look") => InteractionEvent::Polygon(PolygonInteraction::Look),
            Some("action") => InteractionEvent::Polygon(PolygonInteraction::Action),
            Some("walkto") => InteractionEvent::Polygon(PolygonInteraction::WalkTo),
            Some("topic") => InteractionEvent::Topic,
            Some("close_conversation") => InteractionEvent::CloseConversation,
            Some("close_inventory") => InteractionEvent::CloseInventory,
            _ => return Err("invalid interaction event".into()),
        };
        let event = match event {
            InteractionEvent::Polygon(event) if target_key == Some("point") => {
                let point = entry["point"]
                    .as_array()
                    .filter(|v| v.len() == 2)
                    .ok_or("invalid screen point")?;
                let x = i32::try_from(point[0].as_i64().ok_or("invalid point x")?)?;
                let y = i32::try_from(point[1].as_i64().ok_or("invalid point y")?)?;
                if !(0..320).contains(&x) || !(0..200).contains(&y) {
                    return Err("point outside viewport".into());
                }
                InteractionEvent::World([x, y], event)
            }

            InteractionEvent::Polygon(event) if target_key == Some("inventory_slot") => {
                InteractionEvent::Inventory(event)
            }
            InteractionEvent::Polygon(event) if target_key == Some("actor") => {
                if target == 0 || target > 512 {
                    return Err("invalid actor ID".into());
                }
                InteractionEvent::Actor(event)
            }
            event => event,
        };
        if scene == "*" && !matches!(event, InteractionEvent::Move(_)) {
            return Err("scene wildcard is only allowed for cursor movement".into());
        }
        result.push(Interaction {
            tick,
            scene,
            target,
            event,
        });
    }
    Ok(result)
}

/// Reject misspelled scene names before simulating the recorded route.
pub fn validate_scenes(
    inputs: &[Interaction],
    index: &[discworld_core::resource::IndexEntry],
) -> Result<(), Box<dyn std::error::Error>> {
    for input in inputs {
        if input.scene == "*" && matches!(input.event, InteractionEvent::Move(_)) {
            continue;
        }
        if !index.iter().any(|entry| entry.name == input.scene) {
            return Err(format!(
                "unknown replay scene {:?} at tick {}",
                input.scene, input.tick
            )
            .into());
        }
    }
    Ok(())
}

pub fn apply(
    input: &Interaction,
    engine: &mut discworld_core::engine::Engine,
    resources: &discworld_core::graphics::Resources<'_>,
    scene: &str,
) -> Result<bool, discworld_core::iso::Error> {
    let untargeted_move = input.scene == "*" && matches!(input.event, InteractionEvent::Move(_));
    if scene != input.scene && !untargeted_move {
        Err(discworld_core::iso::Error("interaction scene mismatch"))
    } else {
        match input.event {
            InteractionEvent::Move(point) => {
                engine.move_cursor_with_configuration(resources, point)?;
                Ok(true)
            }
            InteractionEvent::Key => engine.user_event().map(|_| true),
            InteractionEvent::Scroll(action) => Ok(engine.scroll_inventory(action)),
            InteractionEvent::CloseInventory => {
                let open = matches!(engine.active_inventory(), Some(1 | 2));
                if open {
                    engine.close_inventory_input(resources)?;
                }
                Ok(open)
            }
            InteractionEvent::World(point, event) => {
                engine.move_cursor(point);
                engine.interact_world(resources, point, event)
            }
            InteractionEvent::Polygon(event) => {
                engine.interact_polygon(resources, input.target, event)
            }
            InteractionEvent::Actor(event) => {
                engine.interact_actor(resources, input.target as u32, event)
            }
            InteractionEvent::Inventory(event) => {
                if let Some(point) = engine.inventory_cell_position(input.target) {
                    engine.move_cursor(point);
                }
                engine.interact_inventory(resources, input.target, event)
            }
            InteractionEvent::Topic => engine.interact_conversation(resources, Some(input.target)),
            InteractionEvent::CloseConversation => engine.interact_conversation(resources, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_samples_allow_no_scene_target_but_keep_action_checks() {
        let moves = interactions(
            br#"[{"tick":1,"scene":"*","event":"move","point":[10,11]}]"#,
            2,
        )
        .unwrap();
        assert_eq!(moves[0].event, InteractionEvent::Move([10, 11]));
        validate_scenes(&moves, &[]).unwrap();
        for bad in [
            br#"[{"tick":1,"scene":"*","event":"key"}]"#.as_slice(),
            br#"[{"tick":1,"scene":"*","event":"action","point":[10,11]}]"#,
            br#"[{"tick":1,"scene":"*","event":"move","point":[320,11]}]"#,
            br#"[{"tick":1,"scene":"*","event":"move","actor":7}]"#,
        ] {
            assert!(interactions(bad, 2).is_err());
        }
        let named = interactions(
            br#"[{"tick":1,"scene":"missing.scn","event":"move","point":[10,11]}]"#,
            2,
        )
        .unwrap();
        assert!(validate_scenes(&named, &[]).is_err());
    }
}
