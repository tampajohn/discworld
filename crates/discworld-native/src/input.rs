//! Host button ownership; all gesture timing stays in the shared engine.
use discworld_core::mouse::Button;
use sdl2::mouse::MouseButton;

#[derive(Default)]
pub struct Buttons([bool; 2]);

impl Buttons {
    pub fn restored_drag(dragging: bool) -> Self {
        Self([dragging, false])
    }
    fn mapped(button: MouseButton) -> Option<(usize, Button)> {
        match button {
            MouseButton::Left => Some((0, Button::Left)),
            MouseButton::Right => Some((1, Button::Right)),
            _ => None,
        }
    }

    pub fn press(&mut self, button: MouseButton) -> Option<Button> {
        let (index, button) = Self::mapped(button)?;
        if std::mem::replace(&mut self.0[index], true) {
            None
        } else {
            Some(button)
        }
    }

    pub fn release(&mut self, button: MouseButton) -> Option<Button> {
        let (index, button) = Self::mapped(button)?;
        if std::mem::replace(&mut self.0[index], false) {
            Some(button)
        } else {
            None
        }
    }

    pub fn any(&self) -> bool {
        self.0.iter().any(|down| *down)
    }

    pub fn release_all(&mut self) -> Vec<Button> {
        [MouseButton::Left, MouseButton::Right]
            .into_iter()
            .filter_map(|button| self.release(button))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdl_queue_keeps_fast_press_release_order_and_event_coordinates() {
        use sdl2::event::Event;
        let sdl = sdl2::init().unwrap();
        let events = sdl.event().unwrap();
        let mut pump = sdl.event_pump().unwrap();
        // Both events arrive before the host next drains its queue.
        events
            .push_event(Event::MouseButtonDown {
                timestamp: 10,
                window_id: 0,
                which: 0,
                mouse_btn: MouseButton::Left,
                clicks: 1,
                x: 118,
                y: 132,
            })
            .unwrap();
        events
            .push_event(Event::MouseButtonUp {
                timestamp: 11,
                window_id: 0,
                which: 0,
                mouse_btn: MouseButton::Left,
                clicks: 1,
                x: 140,
                y: 132,
            })
            .unwrap();
        let mut buttons = Buttons::default();
        let edges: Vec<_> = pump
            .poll_iter()
            .filter_map(|event| match event {
                Event::MouseButtonDown {
                    mouse_btn, x, y, ..
                } => buttons.press(mouse_btn).map(|button| (button, true, x, y)),
                Event::MouseButtonUp {
                    mouse_btn, x, y, ..
                } => buttons
                    .release(mouse_btn)
                    .map(|button| (button, false, x, y)),
                _ => None,
            })
            .collect();
        assert_eq!(
            edges,
            vec![
                (Button::Left, true, 118, 132),
                (Button::Left, false, 140, 132)
            ]
        );
        assert!(!buttons.any());
    }

    #[test]
    fn complete_click_pairs_are_not_collapsed_to_final_button_state() {
        let mut buttons = Buttons::default();
        let mut edges = Vec::new();
        for _ in 0..2 {
            edges.push((buttons.press(MouseButton::Left).unwrap(), true));
            edges.push((buttons.release(MouseButton::Left).unwrap(), false));
        }
        assert_eq!(
            edges,
            vec![
                (Button::Left, true),
                (Button::Left, false),
                (Button::Left, true),
                (Button::Left, false)
            ]
        );
        assert!(!buttons.any());
    }

    #[test]
    fn focus_loss_releases_owned_buttons_once_and_ignores_unowned_releases() {
        let mut buttons = Buttons::default();
        assert_eq!(buttons.release(MouseButton::Left), None);
        assert_eq!(buttons.press(MouseButton::Middle), None);
        buttons.press(MouseButton::Left);
        buttons.press(MouseButton::Right);
        assert_eq!(buttons.press(MouseButton::Left), None);
        assert_eq!(buttons.release_all(), vec![Button::Left, Button::Right]);
        assert_eq!(buttons.release(MouseButton::Right), None);
        assert!(!buttons.any());
    }
}
