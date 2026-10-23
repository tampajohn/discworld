"""Controlled hover diagnostic against the untouched pinned ScummVM binary.

Unlike reference_frames_lldb's passive capture, this probe invokes original
scene-entry/cursor functions. It records every invocation and does not claim
ordinary player-input equivalence. Frame reads use the passive capture helper.
"""
import json
from pathlib import Path
import sys

import lldb

sys.path.insert(0, str(Path(__file__).resolve().parent))
import reference_frames_lldb as frames

START = None
ACTIONS = []
PENDING = []
ROOM = 18 << 23
POINTS = [[50, 120], [147, 110], [294, 56], [250, 160], [149, 60], [10, 10]]


def invoke(frame, expression):
    options = lldb.SBExpressionOptions()
    options.SetIgnoreBreakpoints(True)
    options.SetTimeoutInMicroSeconds(5000000)
    # Give LLDB a concrete result: void expressions can return an unavailable
    # SBValue even when the target function completed.
    value = frame.EvaluateExpression(f'({expression}, 0)', options)
    record = {'cycle': frames.CYCLES, 'expression': expression,
              'error': str(value.GetError())}
    ACTIONS.append(record)
    if value.GetError().Fail():
        raise RuntimeError(f'target diagnostic invocation failed: {record}')


def cycle(frame, location, dictionary):
    global START
    frames.CYCLES += 1
    try:
        if frames.CYCLES == 450:
            PENDING.extend(['Tinsel::EndDw1Intro()', f'Tinsel::SetNewScene({ROOM}u, 1, 1)'])
            return True
        target = frame.GetThread().GetProcess().GetTarget()
        scene = target.FindFirstGlobalVariable('Tinsel::g_SceneHandle')
        active = target.FindFirstGlobalVariable('Tinsel::g_bTagsActive')
        if frames.CYCLES > 450 and START is None:
            if frames.unsigned(scene) == ROOM and frames.unsigned(active):
                START = frames.CYCLES
        if START is not None:
            elapsed = frames.CYCLES - START
            if elapsed % 12 == 0 and elapsed // 12 < len(POINTS):
                x, y = POINTS[elapsed // 12]
                # Set the logical driver position without the dummy SDL backend's
                # warp conversion, then run the original cursor movement logic.
                PENDING.append(f'Tinsel::_vm->_mousePos.x = {x}, '
                               f'Tinsel::_vm->_mousePos.y = {y}, '
                               'Tinsel::_vm->_cursor->DoCursorMove()')
                return True
        if frames.CYCLES >= 4000:
            raise RuntimeError('hover diagnostic cycle budget exhausted')
        return False
    except Exception as error:
        frames.FAILURES.append(str(error))
        return True


def drive(debugger):
    """Run requested actions only after LLDB has finished stopping the target."""
    debugger.SetAsync(False)
    process = debugger.GetSelectedTarget().GetProcess()
    while PENDING and not frames.FAILURES:
        actions = list(PENDING)
        PENDING.clear()
        try:
            frame = process.GetSelectedThread().GetFrameAtIndex(0)
            for expression in actions:
                invoke(frame, expression)
        except Exception as error:
            frames.FAILURES.append(str(error))
            return
        result = process.Continue()
        if result.Fail():
            frames.FAILURES.append(str(result))
            return


def capture(frame, location, dictionary):
    if START is None:
        return False
    result = frames.capture(frame, location, dictionary)
    if frames.RECORDS:
        target = frame.GetThread().GetProcess().GetTarget()
        vm = target.FindFirstGlobalVariable('Tinsel::_vm')
        mouse = vm.Dereference().GetChildMemberWithName('_mousePos')
        if not mouse.IsValid():
            frames.FAILURES.append('driver cursor unavailable')
            return True
        frames.RECORDS[-1]['driver_cursor'] = [
            mouse.GetChildMemberWithName(axis).GetValueAsSigned() for axis in ['x', 'y']]
        for name in ['g_TaggedActor', 'g_hTaggedPolygon']:
            value = target.FindFirstGlobalVariable('Tinsel::' + name)
            if value.IsValid():
                frames.RECORDS[-1][name] = value.GetValueAsSigned()
    return result


def palette(frame, location, dictionary):
    return frames.palette(frame, location, dictionary)


def print_tag(frame, location, dictionary):
    try:
        frames.EVENTS.append({'cycle': frames.CYCLES, 'kind': 'print_tag',
                              'polygon': frame.FindVariable('hp').GetValueAsSigned(),
                              'text': frames.unsigned(frame.FindVariable('text'))})
        return False
    except Exception as error:
        frames.FAILURES.append(str(error))
        return True


def finish(debugger):
    frames.finish(debugger)
    path = frames.OUTPUT / 'capture.json'
    report = json.loads(path.read_text())
    report.update(scope='Controlled original debug scene entry, logical driver writes and original DoCursorMove; not normal host input or full gameplay equivalence.',
                  injected_actions=ACTIONS, hover_start_cycle=START)
    path.write_text(json.dumps(report, indent=2) + '\n')


def __lldb_init_module(debugger, dictionary):
    if frames.BACKEND != 'surfacesdl' or frames.LIMIT != 72:
        raise ValueError('hover probe requires surfacesdl and exactly 72 frames')
    frames.OUTPUT.mkdir()
    target = debugger.GetSelectedTarget()
    points = [
        (target.BreakpointCreateByName('SurfaceSdlGraphicsManager::setPalette'), __name__ + '.palette'),
        (target.BreakpointCreateByLocation('background.cpp', 288), __name__ + '.capture'),
        (target.BreakpointCreateByName('Tinsel::TinselEngine::NextGameCycle'), __name__ + '.cycle'),
        (target.BreakpointCreateByName('Tinsel::PrintTag'), __name__ + '.print_tag'),
    ]
    for bp, callback in points:
        if bp.GetNumLocations() != 1:
            raise RuntimeError(f'nonunique hover breakpoint: {callback}')
        bp.SetScriptCallbackFunction(callback)
