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


def read(process, address, size):
    error = lldb.SBError()
    data = process.ReadMemory(address, size, error)
    if error.Fail() or len(data) != size:
        raise RuntimeError(f'memory read failed: {error}')
    return data


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
        RECORDS.append({'frame': number, 'scene': scene.GetValueAsUnsigned(),
                        'rgb_sha256': hashlib.sha256(rgb).hexdigest(),
                        'indices_sha256': hashlib.sha256(indices).hexdigest(),
                        'palette_sha256': hashlib.sha256(colors).hexdigest()})
        return number >= LIMIT
    except Exception as error:
        FAILURES.append(str(error))
        return True


def finish(debugger):
    report = {'frames': RECORDS, 'errors': FAILURES, 'limit': LIMIT,
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
    if pal.GetNumLocations() != 1 or draw.GetNumLocations() != 1:
        raise RuntimeError('pinned reference breakpoint locations not unique')
    pal.SetScriptCallbackFunction(__name__ + '.palette')
    draw.SetScriptCallbackFunction(__name__ + '.capture')
