"""Read-only frame/palette probe loaded by LLDB into the pinned reference run.

No expressions invoke target code. The launcher owns process startup/termination.
Environment: DISCWORLD_REFERENCE_FRAMES (new output directory),
DISCWORLD_REFERENCE_FRAME_LIMIT (positive frame count, default 400).
DISCWORLD_REFERENCE_BACKEND (opengl or surfacesdl, default opengl).
"""
import hashlib
import json
import os
from pathlib import Path

import lldb

OUTPUT = Path(os.environ['DISCWORLD_REFERENCE_FRAMES'])
LIMIT = int(os.environ.get('DISCWORLD_REFERENCE_FRAME_LIMIT', '400'))
BACKEND = os.environ.get("DISCWORLD_REFERENCE_BACKEND", "opengl")
PALETTE_ADDRESS = None
PALETTE_STRIDE = 3
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


def scroll_event(frame, kind, names, member=False):
    try:
        obj = frame.FindVariable('this').Dereference() if member else None
        record = {'cycle': CYCLES, 'kind': kind}
        for name in names:
            value = obj.GetChildMemberWithName(name) if member else frame.FindVariable(name)
            if not value.IsValid() or value.GetError().Fail():
                raise RuntimeError(f'scroll debug variable unavailable: {name}')
            error = lldb.SBError()
            record[name] = value.GetValueAsSigned(error)
            if error.Fail():
                raise RuntimeError(f'scroll debug value unavailable: {name}: {error}')
        EVENTS.append(record)
        return False
    except Exception as error:
        FAILURES.append(str(error))
        return True


def scroll_to(frame, location, dictionary):
    return scroll_event(frame, 'scroll_to', ['x', 'y', 'xIter', 'yIter'])


def scroll_image(frame, location, dictionary):
    return scroll_event(frame, 'scroll_image', ['_leftScroll', '_downScroll'], True)


def scroll_position(frame, location, dictionary):
    return scroll_event(frame, 'playfield_position', ['which', 'newXpos', 'newYpos'])


def sample_pcm(frame, location, dictionary):
    try:
        size = unsigned(frame.FindVariable('sampleLen'))
        if not 0 < size <= 16 * 1024 * 1024:
            raise RuntimeError('unexpected reference PCM sample size')
        data = read(frame.GetThread().GetProcess(),
                    unsigned(frame.FindVariable('sampleBuf')), size)
        EVENTS.append({'cycle': CYCLES, 'kind': 'sample_pcm',
                       'id': unsigned(frame.FindVariable('id')),
                       'type': unsigned(frame.FindVariable('type')),
                       'bytes': size, 'sha256': hashlib.sha256(data).hexdigest()})
        return False
    except Exception as error:
        FAILURES.append(str(error))
        return True


def samples_stop(frame, location, dictionary):
    EVENTS.append({'cycle': CYCLES, 'kind': 'samples_stop'})
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
    global PALETTE_ADDRESS, PALETTE_STRIDE
    try:
        obj = frame.FindVariable('this').Dereference()
        if BACKEND == 'surfacesdl':
            array = obj.GetChildMemberWithName('_currentPalette')
            PALETTE_ADDRESS = unsigned(array)
            element = array.GetType().GetPointeeType()
            fields = {element.GetFieldAtIndex(i).GetName():
                      element.GetFieldAtIndex(i).GetOffsetInBytes()
                      for i in range(element.GetNumberOfFields())}
            PALETTE_STRIDE = element.GetByteSize()
            if PALETTE_STRIDE != 4 or any(fields.get(c) != i for i, c in enumerate('rgb')):
                raise RuntimeError('unexpected SDL palette layout')
        else:
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
        packed = read(process, PALETTE_ADDRESS, 256 * PALETTE_STRIDE)
        colors = b''.join(packed[i:i+3] for i in range(0, len(packed), PALETTE_STRIDE))
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
    report = {'frames': RECORDS, 'animations': ANIMATIONS, 'steps': STEPS, 'events': EVENTS, 'errors': FAILURES, 'limit': LIMIT, 'graphics_backend': BACKEND,
              'complete': len(RECORDS) == LIMIT and not FAILURES,
              'scope': 'Read-only LLDB capture at DrawBackgnd before updateScreen; frame ordinals are not engine ticks. Debugger pauses alter wall-clock timing. Input determinism is not established.'}
    (OUTPUT / 'capture.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'frames': len(RECORDS), 'errors': FAILURES, 'complete': report['complete']}))


def __lldb_init_module(debugger, dictionary):
    if not 1 <= LIMIT <= 1000:
        raise ValueError('frame limit must be between 1 and 1000')
    if BACKEND not in ('opengl', 'surfacesdl'):
        raise ValueError('unsupported reference graphics backend')
    OUTPUT.mkdir()
    target = debugger.GetSelectedTarget()
    pal = target.BreakpointCreateByName(
        'SurfaceSdlGraphicsManager::setPalette' if BACKEND == 'surfacesdl'
        else 'OpenGLGraphicsManager::setPalette')
    draw = target.BreakpointCreateByLocation('background.cpp', 288)
    tick = target.BreakpointCreateByName('Tinsel::TinselEngine::NextGameCycle')
    anim = target.BreakpointCreateByName('Tinsel::InitStepAnimScript')
    advance = target.BreakpointCreateByName('Tinsel::DoNextFrame')
    scheduled = target.BreakpointCreateByLocation('play.cpp', 1008)
    entered = target.BreakpointCreateByName('Tinsel::t1PlayReel')
    scroll = target.BreakpointCreateByName('Tinsel::Scroll::ScrollTo')
    scroll_step = target.BreakpointCreateByName('Tinsel::Scroll::ScrollImage')
    position = target.BreakpointCreateByName('Tinsel::Background::PlayfieldSetPos')
    pcm = target.BreakpointCreateByLocation('sound.cpp', 165)
    stop = target.BreakpointCreateByName('Tinsel::SoundManager::stopAllSamples')
    if any(bp.GetNumLocations() != 1 for bp in [pal, draw, tick, anim, advance, scheduled, entered, scroll, scroll_step, position, pcm, stop]):
        raise RuntimeError('pinned reference breakpoint locations not unique')
    pcm.SetScriptCallbackFunction(__name__ + '.sample_pcm')
    stop.SetScriptCallbackFunction(__name__ + '.samples_stop')
    scroll.SetScriptCallbackFunction(__name__ + '.scroll_to')
    scroll_step.SetScriptCallbackFunction(__name__ + '.scroll_image')
    position.SetScriptCallbackFunction(__name__ + '.scroll_position')
    scheduled.SetScriptCallbackFunction(__name__ + '.schedule_reel')
    entered.SetScriptCallbackFunction(__name__ + '.enter_reel')
    advance.SetScriptCallbackFunction(__name__ + '.step')
    anim.SetScriptCallbackFunction(__name__ + '.animation')
    tick.SetScriptCallbackFunction(__name__ + '.cycle')
    pal.SetScriptCallbackFunction(__name__ + '.palette')
    draw.SetScriptCallbackFunction(__name__ + '.capture')
