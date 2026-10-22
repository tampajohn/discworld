// SPDX-License-Identifier: GPL-3.0-or-later
//! Tinsel V1 bytecode machine. Semantics follow the pinned ScummVM pcode.cpp.
//! Host library calls suspend explicitly; no engine operation is silently stubbed.
use crate::iso::Error;

const STACK_SIZE: usize = 128;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    pub opcode: u8,
    pub operand: Option<i32>,
    pub next: usize,
}

pub fn decode(code: &[u8], ip: usize) -> Result<Instruction, Error> {
    let byte = *code.get(ip).ok_or(Error("instruction outside script"))?;
    let opcode = byte & 0x3f;
    // CIMM (42) is declared but not implemented by the V1 reference interpreter.
    if opcode > 41 && opcode != 43 {
        return Err(Error("unsupported V1 opcode"));
    }
    let mut next = ip
        .checked_add(1)
        .ok_or(Error("instruction offset overflow"))?;
    let operand = if matches!(opcode, 1 | 5..=14 | 16..=19 | 43) {
        let size = if byte & 0x40 != 0 {
            1
        } else if byte & 0x80 != 0 {
            2
        } else {
            4
        };
        let end = next
            .checked_add(size)
            .ok_or(Error("operand offset overflow"))?;
        let data = code.get(next..end).ok_or(Error("truncated operand"))?;
        next = end;
        Some(match size {
            1 => i32::from(data[0] as i8),
            2 => i32::from(i16::from_le_bytes(data.try_into().unwrap())),
            _ => i32::from_le_bytes(data.try_into().unwrap()),
        })
    } else {
        None
    };
    Ok(Instruction {
        opcode,
        operand,
        next,
    })
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryCall {
    pub id: i32,
    pub instruction: usize,
    pub next: usize,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Continue,
    Library(LibraryCall),
    Halted,
    /// A committed ESCON/ESCOFF instruction. Repeated ESCON still emits an
    /// event so the scheduler can capture a fresh escape generation each time.
    Escape(bool),
}

/// V1 interpreter resume states used by SAVESCENE/NEWSCENE. Unlike V2,
/// RES_1 survives unrelated library calls until explicitly consumed.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
enum SceneResume {
    #[default]
    Normal,
    Restored,
    Saved,
}

/// Explicit interpreter state, included in complete-runtime snapshots.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    #[serde(with = "crate::save_array")]
    stack: [i32; STACK_SIZE],
    sp: usize,
    bp: usize,
    ip: usize,
    halted: bool,
    pending: Option<LibraryCall>,
    scene_resume: SceneResume,
    pub escape_enabled: bool,
}
impl Default for Machine {
    fn default() -> Self {
        Self {
            stack: [0; STACK_SIZE],
            sp: 0,
            bp: 1,
            ip: 0,
            halted: false,
            pending: None,
            scene_resume: SceneResume::Normal,
            escape_enabled: false,
        }
    }
}
impl Machine {
    pub(crate) fn is_halted(&self) -> bool {
        self.halted
    }
    pub(crate) fn validate_snapshot_code(&self, code: &[u8]) -> Result<(), Error> {
        self.validate_snapshot_structure()?;
        if self.halted {
            if self.ip == 0 || self.ip > code.len() || code[self.ip - 1] & 0x3f != 0 {
                return Err(Error("snapshot halted position does not match bytecode"));
            }
        } else {
            let instruction = decode(code, self.ip)?;
            if let Some(call) = self.pending {
                if instruction.opcode != 14
                    || instruction.operand != Some(call.id)
                    || instruction.next != call.next
                {
                    return Err(Error("snapshot pending call does not match bytecode"));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn validate_snapshot_structure(&self) -> Result<(), Error> {
        if self.sp >= STACK_SIZE || self.bp == 0 || self.bp >= STACK_SIZE {
            return Err(Error("invalid snapshot script stack"));
        }
        if self.halted && (self.sp != 0 || self.pending.is_some()) {
            return Err(Error("invalid snapshot halted script"));
        }
        if self.pending.is_some_and(|call| {
            call.id < 0 || call.instruction != self.ip || call.next <= call.instruction
        }) {
            return Err(Error("invalid snapshot pending call"));
        }
        Ok(())
    }

    /// Restore only interpreter context, as pcode.cpp RestoreInterpretContext
    /// does. Host coroutine locals and scene objects must be rebuilt separately.
    pub fn restored_context(&self) -> Self {
        let mut restored = self.clone();
        restored.scene_resume = SceneResume::Restored;
        restored
    }

    /// Complete the two library calls whose V1 behavior changes on restoration.
    /// A restored SAVESCENE arms suppression of the subsequent NEWSCENE so the
    /// interrupted script continues instead of entering its cutaway again.
    pub fn complete_scene_resume(&mut self, call: LibraryCall) -> Result<bool, Error> {
        let (consumed, state) = match (call.id, self.scene_resume) {
            (58, SceneResume::Restored) => (0, SceneResume::Saved),
            (44, SceneResume::Saved) => (3, SceneResume::Normal),
            _ => return Ok(false),
        };
        self.complete_library(call, consumed, None)?;
        self.scene_resume = state;
        Ok(true)
    }

    pub fn pending_call(&self) -> Option<LibraryCall> {
        self.pending
    }
    pub fn ip(&self) -> usize {
        self.ip
    }
    pub fn operands(&self) -> &[i32] {
        &self.stack[1..=self.sp]
    }
    fn push(&mut self, value: i32) -> Result<(), Error> {
        if self.sp + 1 >= STACK_SIZE {
            return Err(Error("script stack overflow"));
        }
        self.sp += 1;
        self.stack[self.sp] = value;
        Ok(())
    }
    fn pop(&mut self) -> Result<i32, Error> {
        if self.sp == 0 {
            return Err(Error("script stack underflow"));
        }
        let value = self.stack[self.sp];
        self.sp -= 1;
        Ok(value)
    }
    fn local(&self, offset: i32) -> Result<usize, Error> {
        let n = (self.bp as i64) + i64::from(offset);
        if !(0..STACK_SIZE as i64).contains(&n) {
            return Err(Error("local outside stack"));
        }
        Ok(n as usize)
    }
    fn target(value: i32, code: &[u8]) -> Result<usize, Error> {
        let n = usize::try_from(value).map_err(|_| Error("negative script target"))?;
        if n >= code.len() {
            return Err(Error("script target outside code"));
        }
        Ok(n)
    }
    /// Errors leave both VM and globals unchanged. One step gives the host a finite budget.
    pub fn step(&mut self, code: &[u8], globals: &mut [i32]) -> Result<Step, Error> {
        if self.halted {
            return Ok(Step::Halted);
        }
        if let Some(call) = self.pending {
            return Ok(Step::Library(call));
        }
        let mut next = self.clone();
        let (result, write) = next.execute(code, globals)?;
        if let Some((index, value)) = write {
            globals[index] = value;
        }
        *self = next;
        Ok(result)
    }
    fn execute(
        &mut self,
        code: &[u8],
        globals: &[i32],
    ) -> Result<(Step, Option<(usize, i32)>), Error> {
        let ins = decode(code, self.ip)?;
        let arg = ins.operand.unwrap_or(0);
        let origin = self.ip;
        self.ip = ins.next;
        let mut write = None;
        match ins.opcode {
            0 => {
                if self.sp != 0 {
                    return Err(Error("halt with nonempty stack"));
                }
                self.halted = true;
                return Ok((Step::Halted, None));
            }
            1 | 5..=8 | 43 => self.push(arg)?,
            2 => self.push(0)?,
            3 => self.push(1)?,
            4 => self.push(-1)?,
            9 => {
                let at = self.local(arg)?;
                self.push(self.stack[at])?;
            }
            10 => {
                let at = usize::try_from(arg).map_err(|_| Error("negative global index"))?;
                self.push(*globals.get(at).ok_or(Error("global outside table"))?)?;
            }
            11 => {
                let at = self.local(arg)?;
                self.stack[at] = self.pop()?;
            }
            12 => {
                let at = usize::try_from(arg).map_err(|_| Error("negative global index"))?;
                globals.get(at).ok_or(Error("global outside table"))?;
                write = Some((at, self.pop()?));
            }
            13 => {
                let target = Self::target(arg, code)?;
                if self.sp + 3 >= STACK_SIZE {
                    return Err(Error("call frame outside stack"));
                }
                self.stack[self.sp + 1] = 0;
                self.stack[self.sp + 2] = self.bp as i32;
                self.stack[self.sp + 3] =
                    i32::try_from(ins.next).map_err(|_| Error("return address overflow"))?;
                self.bp = self.sp + 1;
                self.ip = target;
            }
            14 => {
                if arg < 0 {
                    return Err(Error("negative library call"));
                }
                let call = LibraryCall {
                    id: arg,
                    instruction: origin,
                    next: ins.next,
                };
                self.pending = Some(call);
                self.ip = origin;
                return Ok((Step::Library(call), None));
            }
            15 => {
                if self.bp == 0 || self.bp + 2 >= STACK_SIZE {
                    return Err(Error("invalid return frame"));
                }
                self.ip = Self::target(self.stack[self.bp + 2], code)?;
                let bp = usize::try_from(self.stack[self.bp + 1])
                    .map_err(|_| Error("invalid saved base pointer"))?;
                if bp == 0 || bp >= STACK_SIZE {
                    return Err(Error("invalid saved base pointer"));
                }
                self.sp = self.bp - 1;
                self.bp = bp;
            }
            16 => {
                let sp = self.sp as i64 + i64::from(arg);
                if !(0..STACK_SIZE as i64).contains(&sp) {
                    return Err(Error("stack allocation out of range"));
                }
                self.sp = sp as usize;
            }
            17 => self.ip = Self::target(arg, code)?,
            18 | 19 => {
                let value = self.pop()?;
                if (value != 0) == (ins.opcode == 19) {
                    self.ip = Self::target(arg, code)?;
                }
            }
            20..=35 => {
                let right = self.pop()?;
                let left = self.pop()?;
                let value = match ins.opcode {
                    20 => i32::from(left == right),
                    21 => i32::from(left < right),
                    22 => i32::from(left <= right),
                    23 => i32::from(left != right),
                    24 => i32::from(left >= right),
                    25 => i32::from(left > right),
                    26 => left.wrapping_add(right),
                    27 => left.wrapping_sub(right),
                    28 => i32::from(left != 0 || right != 0),
                    29 => left.wrapping_mul(right),
                    30 => left.checked_div(right).ok_or(Error("invalid division"))?,
                    31 => left.checked_rem(right).ok_or(Error("invalid remainder"))?,
                    32 => left & right,
                    33 => left | right,
                    34 => left ^ right,
                    35 => i32::from(left != 0 && right != 0),
                    _ => unreachable!(),
                };
                self.push(value)?;
            }
            36..=39 => {
                let value = self.pop()?;
                self.push(match ins.opcode {
                    36 => i32::from(value == 0),
                    37 => !value,
                    38 => value.wrapping_neg(),
                    _ => value,
                })?;
                if ins.opcode == 39 {
                    self.push(value)?;
                }
            }
            40 | 41 => {
                self.escape_enabled = ins.opcode == 40;
                return Ok((Step::Escape(self.escape_enabled), None));
            }
            _ => return Err(Error("unsupported V1 opcode")),
        }
        Ok((Step::Continue, write))
    }
    /// Complete a library operation exactly once. A suspended operation leaves operands intact.
    pub fn complete_library(
        &mut self,
        call: LibraryCall,
        consumed: usize,
        result: Option<i32>,
    ) -> Result<(), Error> {
        if self.pending != Some(call) {
            return Err(Error("library completion does not match pending call"));
        }
        if consumed > self.sp {
            return Err(Error("library argument underflow"));
        }
        let mut next = self.clone();
        next.sp -= consumed;
        if let Some(value) = result {
            next.push(value)?;
        }
        next.pending = None;
        next.ip = call.next;
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escape_instructions_emit_each_committed_transition_once() {
        let code = [40, 40, 0x4e, 86, 41, 0];
        let mut vm = Machine::default();
        assert_eq!(vm.step(&code, &mut []).unwrap(), Step::Escape(true));
        assert!(vm.escape_enabled);
        let saved = postcard::to_allocvec(&vm).unwrap();
        let mut restored: Machine = postcard::from_bytes(&saved).unwrap();
        assert_eq!(restored, vm);
        // An already-enabled ESCON must be observable, not only Boolean edges.
        for machine in [&mut vm, &mut restored] {
            assert_eq!(machine.step(&code, &mut []).unwrap(), Step::Escape(true));
            let Step::Library(call) = machine.step(&code, &mut []).unwrap() else {
                panic!("expected suspended library call");
            };
            assert_eq!(machine.step(&code, &mut []).unwrap(), Step::Library(call));
            assert!(machine.escape_enabled);
            machine.complete_library(call, 0, None).unwrap();
            assert_eq!(machine.step(&code, &mut []).unwrap(), Step::Escape(false));
            assert!(!machine.escape_enabled);
            assert_eq!(machine.step(&code, &mut []).unwrap(), Step::Halted);
            assert_eq!(machine.step(&code, &mut []).unwrap(), Step::Halted);
        }
        assert_eq!(vm, restored);
    }

    #[test]
    fn snapshot_pending_and_halted_positions_match_original_code() {
        let code = [0x4e, 86, 0];
        let mut vm = Machine::default();
        vm.step(&code, &mut []).unwrap();
        assert!(vm.validate_snapshot_code(&code).is_ok());
        assert!(vm.validate_snapshot_code(&[0x4e, 85, 0]).is_err());
        assert!(vm.validate_snapshot_code(&[0x41, 86, 0]).is_err());
        assert!(vm.validate_snapshot_code(&[0x4e]).is_err());
        let mut invalid = vm.clone();
        invalid.pending.as_mut().unwrap().next = 3;
        assert!(invalid.validate_snapshot_code(&code).is_err());
        vm.complete_library(vm.pending_call().unwrap(), 0, None)
            .unwrap();
        assert!(vm.validate_snapshot_code(&code).is_ok());
        vm.step(&code, &mut []).unwrap();
        assert!(vm.validate_snapshot_code(&code).is_ok());
        assert!(vm.validate_snapshot_code(&[0x4e, 86, 2]).is_err());
        vm.ip = usize::MAX;
        assert!(vm.validate_snapshot_code(&code).is_err());
    }

    #[test]
    fn snapshot_rejects_invalid_stack_and_pending_state() {
        let valid = Machine::default();
        assert!(valid.validate_snapshot_structure().is_ok());
        for sp in [STACK_SIZE, usize::MAX] {
            let mut invalid = valid.clone();
            invalid.sp = sp;
            assert!(invalid.validate_snapshot_structure().is_err());
        }
        for bp in [0, STACK_SIZE, usize::MAX] {
            let mut invalid = valid.clone();
            invalid.bp = bp;
            assert!(invalid.validate_snapshot_structure().is_err());
        }
        let mut pending = valid.clone();
        pending.step(&[0x4e, 86], &mut []).unwrap();
        assert!(pending.validate_snapshot_structure().is_ok());
        for call in [
            LibraryCall {
                id: -1,
                instruction: 0,
                next: 2,
            },
            LibraryCall {
                id: 86,
                instruction: 1,
                next: 2,
            },
            LibraryCall {
                id: 86,
                instruction: 0,
                next: 0,
            },
        ] {
            pending.pending = Some(call);
            assert!(pending.validate_snapshot_structure().is_err());
        }
        pending.pending = Some(LibraryCall {
            id: 86,
            instruction: 0,
            next: 2,
        });
        pending.halted = true;
        assert!(pending.validate_snapshot_structure().is_err());
        let mut halted = valid;
        halted.step(&[0], &mut []).unwrap();
        assert!(halted.validate_snapshot_structure().is_ok());
        halted.sp = 1;
        assert!(halted.validate_snapshot_structure().is_err());
    }

    #[test]
    fn restored_scene_resume_is_atomic_and_consumes_one_cutaway() {
        let mut vm = Machine::default().restored_context();
        let code = [0x4e, 58, 0x4e, 44];
        let Step::Library(save) = vm.step(&code, &mut []).unwrap() else {
            panic!()
        };
        assert!(vm.complete_scene_resume(save).unwrap());
        let Step::Library(change) = vm.step(&code, &mut []).unwrap() else {
            panic!()
        };
        let before = vm.clone();
        assert!(vm.complete_scene_resume(change).is_err());
        assert_eq!(vm, before);
        for value in [123, 4, 1] {
            vm.push(value).unwrap();
        }
        assert!(vm.complete_scene_resume(change).unwrap());
        assert!(vm.operands().is_empty());
        assert_eq!(vm.scene_resume, SceneResume::Normal);
        assert!(!vm.complete_scene_resume(change).unwrap());
        // A saved context already marked RES_2 is restored as RES_1 again.
        assert_eq!(
            before.restored_context().scene_resume,
            SceneResume::Restored
        );
        assert_eq!(before.scene_resume, SceneResume::Saved);
    }

    #[test]
    fn division_error_is_atomic_and_arithmetic_wraps() {
        let mut vm = Machine::default();
        let mut globals = [];
        let code = [3, 2, 30];
        vm.step(&code, &mut globals).unwrap();
        vm.step(&code, &mut globals).unwrap();
        let before = vm.clone();
        assert!(vm.step(&code, &mut globals).is_err());
        assert_eq!(vm, before);
        let code = [1, 255, 255, 255, 127, 3, 26, 0x4c, 0, 0];
        let mut globals = [0];
        run(&code, &mut globals).unwrap();
        assert_eq!(globals, [i32::MIN]);
    }
    #[test]
    fn negative_local_offsets_access_call_arguments() {
        let code = [
            0x41, 42, 0x4d, 7, 0x50, 255, 0, 0x50, 3, 0x49, 255, 0x4c, 0, 15,
        ];
        let mut globals = [0];
        run(&code, &mut globals).unwrap();
        assert_eq!(globals, [42]);
    }
    fn run(code: &[u8], globals: &mut [i32]) -> Result<Machine, Error> {
        let mut vm = Machine::default();
        for _ in 0..100 {
            if vm.step(code, globals)? == Step::Halted {
                return Ok(vm);
            }
        }
        Err(Error("test budget exceeded"))
    }
    #[test]
    fn signed_operands_and_width_precedence() {
        assert_eq!(decode(&[0xc1, 255], 0).unwrap().operand, Some(-1));
        assert_eq!(decode(&[0x81, 0, 128], 0).unwrap().operand, Some(-32768));
        assert!(decode(&[0x81, 0], 0).is_err());
        assert!(decode(&[42], 0).is_err());
    }
    #[test]
    fn arithmetic_and_globals() {
        let mut globals = [0];
        run(&[0x41, 7, 0x41, 3, 27, 0x4c, 0, 0], &mut globals).unwrap();
        assert_eq!(globals, [4]);
    }
    #[test]
    fn absolute_branch() {
        let mut globals = [0];
        run(
            &[3, 0x53, 6, 0x41, 99, 0, 0x41, 7, 0x4c, 0, 0],
            &mut globals,
        )
        .unwrap();
        assert_eq!(globals, [7]);
    }
    #[test]
    fn call_frame_alloc_and_return() {
        // Call does not raise SP; ALLOC 3 claims the prewritten frame.
        let mut globals = [0];
        run(&[0x4d, 3, 0, 0x50, 3, 0x41, 9, 0x4c, 0, 15], &mut globals).unwrap();
        assert_eq!(globals, [9]);
    }
    #[test]
    fn library_suspension_and_snapshot_resume() {
        let code = [0x41, 7, 0x4e, 5, 0x4c, 0, 0];
        let mut vm = Machine::default();
        let mut globals = [0];
        vm.step(&code, &mut globals).unwrap();
        let Step::Library(call) = vm.step(&code, &mut globals).unwrap() else {
            panic!()
        };
        assert_eq!(vm.operands(), [7]);
        let mut restored = vm.clone();
        assert_eq!(
            restored.step(&code, &mut globals).unwrap(),
            Step::Library(call)
        );
        restored.complete_library(call, 1, Some(42)).unwrap();
        assert!(restored.complete_library(call, 0, None).is_err());
        restored.step(&code, &mut globals).unwrap();
        assert_eq!(globals, [42]);
        assert_eq!(restored.step(&code, &mut globals).unwrap(), Step::Halted);
    }
    #[test]
    fn error_does_not_partially_pop_or_write() {
        let mut vm = Machine::default();
        let mut globals = [0];
        vm.step(&[3], &mut globals).unwrap();
        let before = vm.clone();
        assert!(vm.step(&[3, 26], &mut globals).is_err());
        assert_eq!(vm, before);
        assert_eq!(globals, [0]);
    }
    #[test]
    fn rejects_unbalanced_halt_and_stack_overflow() {
        assert!(run(&[3, 0], &mut []).is_err());
        let mut vm = Machine::default();
        let code = vec![3; 128];
        for _ in 0..127 {
            vm.step(&code, &mut []).unwrap();
        }
        assert!(vm.step(&code, &mut []).is_err());
    }
    #[test]
    fn empty_operand_view() {
        assert!(Machine::default().operands().is_empty());
    }
}
