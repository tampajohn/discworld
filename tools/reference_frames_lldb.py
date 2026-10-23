"""Read-only frame/palette probe loaded by LLDB into the pinned reference run.

No expressions invoke target code. The launcher owns process startup/termination.
Environment: DISCWORLD_REFERENCE_FRAMES (new output directory),
DISCWORLD_REFERENCE_FRAME_LIMIT (positive frame count, default 400).
"""
import hashlib
import json
import os
from pathlib import Path

import lldb

OUTPUT = Path(os.environ['DISCWORLD_REFERENCE_FRAMES'])
LIMIT = int(os.environ.get('DISCWORLD_REFERENCE_FRAME_LIMIT', '400'))
PALETTE_ADDRESS = None
RECORDS = []
FAILURES = []
CYCLES = 0
ANIMATIONS = []
STEPS = []
EVENTS = []


def unsigned(value):
    if not value.IsValid() or value.GetError().Fail():
        raise RuntimeError(f'debug variable unavailable: {value.GetName()}')
    error = lldb.SBError()
    result = value.GetValueAsUnsigned(error)
    if error.Fail():
        raise RuntimeError(f'debug value unavailable: {error}')
    return result


def film_event(frame, kind, pointer):
    try:
        state = frame.FindVariable('ppi')
        if pointer:
            state = state.Dereference()
        EVENTS.append({'cycle': CYCLES, 'kind': kind,
                       'film': unsigned(state.GetChildMemberWithName('hFilm')),
                       'column': unsigned(state.GetChildMemberWithName('column'))})
        return False
    except Exception as error:
        FAILURES.append(str(error))
        return True


def schedule_reel(frame, location, dictionary):
    return film_event(frame, 'schedule_reel', False)


def enter_reel(frame, location, dictionary):
    return film_event(frame, 'enter_reel', True)


def read(process, address, size):
    error = lldb.SBError()
    data = process.ReadMemory(address, size, error)
    if error.Fail() or len(data) != size:
        raise RuntimeError(f'memory read failed: {error}')
    return data


def cycle(frame, location, dictionary):
    global CYCLES
    CYCLES += 1
    return False


def animation(frame, location, dictionary):
    try:
        record = {'cycle': CYCLES,
                  'script': unsigned(frame.FindVariable('hNewScript')),
                  'period': unsigned(frame.FindVariable('aniSpeed'))}
        ANIMATIONS.append(record)
        EVENTS.append(dict(record, kind='init_animation'))
        return False
    except Exception as error:
        FAILURES.append(str(error))
        return True


def step(frame, location, dictionary):
    try:
        state = frame.FindVariable('pAnim').Dereference()
        record = {'cycle': CYCLES,
                  'script': unsigned(state.GetChildMemberWithName('hScript')),
                  'index': unsigned(state.GetChildMemberWithName('scriptIndex'))}
        STEPS.append(record)
        EVENTS.append(dict(record, kind='step_animation'))
        return False
    except Exception as error:
        FAILURES.append(str(error))
        return True


def palette(frame, location, dictionary):
    global PALETTE_ADDRESS
    try:
        obj = frame.FindVariable('this').Dereference()
        array = obj.GetChildMemberWithName('_gamePalette')
        PALETTE_ADDRESS = array.GetLoadAddress()
        if PALETTE_ADDRESS == lldb.LLDB_INVALID_ADDRESS:
            raise RuntimeError('palette member unavailable')
        return False
    except Exception as error:
        FAILURES.append(str(error))
        return True


def capture(frame, location, dictionary):
    try:
        target = frame.GetThread().GetProcess().GetTarget()
        vm = target.FindFirstGlobalVariable('Tinsel::_vm').Dereference()
        surface = vm.GetChildMemberWithName('_screenSurface')
        width = surface.GetChildMemberWithName('w').GetValueAsUnsigned()
        height = surface.GetChildMemberWithName('h').GetValueAsUnsigned()
        pitch = surface.GetChildMemberWithName('pitch').GetValueAsUnsigned()
        pixels = surface.GetChildMemberWithName('pixels').GetValueAsUnsigned()
        bpp = surface.GetChildMemberWithName('format').GetChildMemberWithName('bytesPerPixel').GetValueAsUnsigned()
        if (width, height, bpp) != (320, 200, 1) or not width <= pitch <= 4096:
            raise RuntimeError(f'unexpected framebuffer: {width}x{height}, pitch={pitch}, bpp={bpp}')
        if PALETTE_ADDRESS is None:
            return False
        process = frame.GetThread().GetProcess()
        padded = read(process, pixels, pitch * height)
        indices = b''.join(padded[y*pitch:y*pitch+width] for y in range(height))
        colors = read(process, PALETTE_ADDRESS, 768)
        rgb = b''.join(colors[index*3:index*3+3] for index in indices)
        scene = target.FindFirstGlobalVariable('Tinsel::g_SceneHandle')
        if not scene.IsValid():
            raise RuntimeError('scene handle unavailable')
        number = len(RECORDS) + 1
        stem = f'frame-{number:04d}'
        (OUTPUT / (stem + '.idx')).write_bytes(indices)
        (OUTPUT / (stem + '.pal')).write_bytes(colors)
        RECORDS.append({'frame': number, 'cycle': CYCLES, 'scene': scene.GetValueAsUnsigned(),
                        'rgb_sha256': hashlib.sha256(rgb).hexdigest(),
                        'indices_sha256': hashlib.sha256(indices).hexdigest(),
                        'palette_sha256': hashlib.sha256(colors).hexdigest()})
        return number >= LIMIT
    except Exception as error:
        FAILURES.append(str(error))
        return True


def finish(debugger):
    report = {'frames': RECORDS, 'animations': ANIMATIONS, 'steps': STEPS, 'events': EVENTS, 'errors': FAILURES, 'limit': LIMIT,
              'complete': len(RECORDS) == LIMIT and not FAILURES,
              'scope': 'Read-only LLDB capture at DrawBackgnd before updateScreen; frame ordinals are not engine ticks. Debugger pauses alter wall-clock timing. Input determinism is not established.'}
    (OUTPUT / 'capture.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'frames': len(RECORDS), 'errors': FAILURES, 'complete': report['complete']}))


def __lldb_init_module(debugger, dictionary):
    if not 1 <= LIMIT <= 1000:
        raise ValueError('frame limit must be between 1 and 1000')
    OUTPUT.mkdir()
    target = debugger.GetSelectedTarget()
    pal = target.BreakpointCreateByName('OpenGLGraphicsManager::setPalette')
    draw = target.BreakpointCreateByLocation('background.cpp', 288)
    tick = target.BreakpointCreateByName('Tinsel::TinselEngine::NextGameCycle')
    anim = target.BreakpointCreateByName('Tinsel::InitStepAnimScript')
    advance = target.BreakpointCreateByName('Tinsel::DoNextFrame')
    scheduled = target.BreakpointCreateByLocation('play.cpp', 1008)
    entered = target.BreakpointCreateByName('Tinsel::t1PlayReel')
    if any(bp.GetNumLocations() != 1 for bp in [pal, draw, tick, anim, advance, scheduled, entered]):
        raise RuntimeError('pinned reference breakpoint locations not unique')
    scheduled.SetScriptCallbackFunction(__name__ + '.schedule_reel')
    entered.SetScriptCallbackFunction(__name__ + '.enter_reel')
    advance.SetScriptCallbackFunction(__name__ + '.step')
    anim.SetScriptCallbackFunction(__name__ + '.animation')
    tick.SetScriptCallbackFunction(__name__ + '.cycle')
    pal.SetScriptCallbackFunction(__name__ + '.palette')
    draw.SetScriptCallbackFunction(__name__ + '.capture')
