// SPDX-License-Identifier: GPL-3.0-or-later
//! English V1 mixing-desk slider arithmetic (dialogs.cpp::slideMSlider).
use crate::iso::Error;

pub const RANGE: i64 = 81;

pub fn offset(value: u16, maximum: u16) -> Result<i32, Error> {
    if maximum == 0 || value > maximum {
        return Err(Error("invalid menu slider value"));
    }
    Ok((RANGE * i64::from(value) / i64::from(maximum)) as i32)
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Drag {
    maximum: u16,
    value: u16,
    raw: i64,
    shown: i32,
}
impl Drag {
    pub fn validate_snapshot(&self) -> Result<(), Error> {
        let canonical = offset(self.value, self.maximum)?;
        if i64::from(self.shown) != self.raw.clamp(0, RANGE)
            || (self.shown != canonical
                && self.value != (i64::from(self.shown) * i64::from(self.maximum) / RANGE) as u16)
        {
            return Err(Error("invalid menu slider snapshot"));
        }
        Ok(())
    }
    pub fn new(value: u16, maximum: u16) -> Result<Self, Error> {
        let shown = offset(value, maximum)?;
        Ok(Self {
            maximum,
            value,
            raw: i64::from(shown),
            shown,
        })
    }
    pub fn position(&self) -> i32 {
        self.shown
    }
    pub fn value(&self) -> u16 {
        self.value
    }
    pub(crate) fn maximum(&self) -> u16 {
        self.maximum
    }

    /// The raw coordinate retains overshoot; reversing a clamped drag must
    /// traverse it before the thumb moves back. No movement preserves the
    /// original value even when its initial pixel is a lossy representation.
    /// Some(value) means the thumb moved, so MUSICVOL must apply the value.
    pub fn move_by(&mut self, delta: i32) -> Result<Option<u16>, Error> {
        let raw = self
            .raw
            .checked_add(i64::from(delta))
            .ok_or(Error("menu slider drag overflow"))?;
        let shown = raw.clamp(0, RANGE) as i32;
        self.raw = raw;
        if shown == self.shown {
            return Ok(None);
        }
        self.shown = shown;
        self.value = (i64::from(shown) * i64::from(self.maximum) / RANGE) as u16;
        Ok(Some(self.value))
    }
    /// S_END rebuilds all slider objects from their quantized setting values.
    pub fn released_position(&self) -> i32 {
        (RANGE * i64::from(self.value) / i64::from(self.maximum)) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshots_preserve_quantization_and_raw_drag_history() {
        for maximum in [30, 100, 255] {
            for value in 0..=maximum {
                let mut drag = Drag::new(value, maximum).unwrap();
                for delta in [0, 1, 100, -60, -1, -200, 1, 200] {
                    drag.validate_snapshot().unwrap();
                    let bytes = postcard::to_allocvec(&drag).unwrap();
                    let mut copy: Drag = postcard::from_bytes(&bytes).unwrap();
                    copy.validate_snapshot().unwrap();
                    assert_eq!(copy.move_by(delta), drag.move_by(delta));
                    assert_eq!(copy, drag);
                    assert_eq!(copy.released_position(), drag.released_position());
                }
            }
        }
        for invalid in [
            Drag {
                maximum: 0,
                value: 0,
                raw: 0,
                shown: 0,
            },
            Drag {
                maximum: 30,
                value: 31,
                raw: 81,
                shown: 81,
            },
            Drag {
                maximum: 30,
                value: 0,
                raw: 0,
                shown: -1,
            },
            Drag {
                maximum: 30,
                value: 15,
                raw: 0,
                shown: 0,
            },
        ] {
            assert!(invalid.validate_snapshot().is_err());
        }
    }
    #[test]
    fn drag_retains_overshoot_and_only_updates_when_thumb_moves() {
        let mut drag = Drag::new(128, 255).unwrap();
        assert_eq!(drag.position(), 40);
        assert_eq!(drag.move_by(0).unwrap(), None);
        assert_eq!(drag.value(), 128);
        assert_eq!(drag.move_by(1).unwrap(), Some(129));
        assert_eq!(drag.position(), 41);
        assert_eq!(drag.released_position(), 40);
        assert_eq!(drag.move_by(100).unwrap(), Some(255));
        assert_eq!(drag.move_by(-60).unwrap(), None); // raw now81
        assert_eq!(drag.move_by(-1).unwrap(), Some(251));
        assert_eq!(drag.position(), 80);
        assert_eq!(drag.move_by(-100).unwrap(), Some(0));
        assert_eq!(drag.move_by(20).unwrap(), None);
        assert_eq!(drag.move_by(1).unwrap(), Some(3));
    }
    #[test]
    fn ranges_and_overflow_are_checked_without_partial_change() {
        assert!(Drag::new(0, 0).is_err());
        assert!(Drag::new(256, 255).is_err());
        let mut drag = Drag::new(50, 100).unwrap();
        drag.raw = i64::MAX;
        let before = drag.clone();
        assert!(drag.move_by(1).is_err());
        assert_eq!(drag, before);
        assert_eq!(offset(0, 100).unwrap(), 0);
        assert_eq!(offset(100, 100).unwrap(), 81);
    }
}
