"""Controlled hover diagnostic against the untouched pinned ScummVM binary.

Unlike reference_frames_lldb's passive capture, this probe invokes original
scene-entry/cursor functions. It records every invocation and does not claim
ordinary player-input equivalence. Frame reads use the passive capture helper.
"""
import json
import struct
import hashlib
import os
from pathlib import Path
import sys

import lldb

sys.path.insert(0, str(Path(__file__).resolve().parent))
import reference_frames_lldb as frames

INVENTORY = os.environ.get("DISCWORLD_REFERENCE_INVENTORY") == "1"
INVENTORY_DRAG = os.environ.get("DISCWORLD_REFERENCE_INVENTORY_DRAG") == "1"
INVENTORY_FIXED_CORNER = os.environ.get("DISCWORLD_REFERENCE_INVENTORY_FIXED_CORNER") == "1"
INVENTORY_RESIZE = os.environ.get("DISCWORLD_REFERENCE_INVENTORY_RESIZE") == "1"
RESIZE_CORNER = os.environ.get("DISCWORLD_REFERENCE_RESIZE_CORNER", "bottom-right")
RESIZE_DELTA = int(os.environ.get("DISCWORLD_REFERENCE_RESIZE_DELTA", "26"))
DOOR_EXIT = os.environ.get("DISCWORLD_REFERENCE_DOOR_EXIT") == "1"
UNIVERSITY_WALK = os.environ.get("DISCWORLD_REFERENCE_UNIVERSITY_WALK") == "1"
UNIVERSITY_RETARGET = os.environ.get("DISCWORLD_REFERENCE_UNIVERSITY_RETARGET") == "1"
ARCH_ENTRY = os.environ.get("DISCWORLD_REFERENCE_ARCH_ENTRY") == "1"
ARCH_INVENTORY = os.environ.get("DISCWORLD_REFERENCE_ARCH_INVENTORY") == "1"
ARCH_SKIP = os.environ.get("DISCWORLD_REFERENCE_ARCH_SKIP") == "1"
DOOR_ACTION = os.environ.get("DISCWORLD_REFERENCE_DOOR_ACTION") == "1"
LOOK = os.environ.get("DISCWORLD_REFERENCE_LOOK") == "1"
WALK = os.environ.get("DISCWORLD_REFERENCE_WALK") == "1"
WALK_FROM_ACTOR = os.environ.get("DISCWORLD_REFERENCE_WALK_FROM_ACTOR") == "1"
WALK_DELAY = int(os.environ.get("DISCWORLD_REFERENCE_WALK_DELAY", "1"))
TRACE_CURSOR_ORDER = os.environ.get("DISCWORLD_REFERENCE_TRACE_CURSOR_ORDER") == "1"
TRACE_BACKDROP = os.environ.get("DISCWORLD_REFERENCE_TRACE_BACKDROP") == "1"
BACKDROP_PENDING = None
BACKDROPS = []
START = None
ACTIONS = []
PENDING = []
ROOM = 18 << 23
POINTS = [[50, 120], [147, 110], [294, 56], [250, 160], [149, 60], [10, 10]]

if WALK:
    POINTS[0] = [200, 140] if WALK_FROM_ACTOR else [250, 180]

if INVENTORY_DRAG:
    POINTS = [[50, 120], [140, 110], [170, 130], [180, 135], [10, 10], [10, 10]]

if INVENTORY_FIXED_CORNER:
    POINTS = [[50, 120], [169, 147], [189, 167], [189, 167], [10, 10], [10, 10]]

if INVENTORY_RESIZE:
    corner = [100, 100] if RESIZE_CORNER == 'top-left' else [194, 172]
    direction = -1 if RESIZE_CORNER == 'top-left' else 1
    moved = [p + direction * RESIZE_DELTA for p in corner]
    POINTS = [[50, 120], corner, moved, moved, [10, 10], [10, 10]]


def invoke(frame, expression):
    options = lldb.SBExpressionOptions()
    options.SetIgnoreBreakpoints(True)
    options.SetTimeoutInMicroSeconds(5000000)
    # Give LLDB a concrete result: void expressions can return an unavailable
    # SBValue even when the target function completed.
    before = None
    if 'Tinsel::PlayerEvent(' in expression:
        target = frame.GetThread().GetProcess().GetTarget()
        before = {}
        for name in ['g_TaggedActor', 'g_hTaggedPolygon']:
            value = target.FindFirstGlobalVariable('Tinsel::' + name)
            if not value.IsValid() or value.GetError().Fail():
                raise RuntimeError('reference click target unavailable: ' + name)
            before[name] = value.GetValueAsSigned()
    value = frame.EvaluateExpression(f'({expression}, 0)', options)
    record = {'cycle': frames.CYCLES, 'expression': expression,
              'error': str(value.GetError())}
    if before is not None:
        record['target_before_dispatch'] = before
    ACTIONS.append(record)
    if value.GetError().Fail():
        raise RuntimeError(f'target diagnostic invocation failed: {record}')


def cycle(frame, location, dictionary):
    global START
    frames.CYCLES += 1
    try:
        if ARCH_INVENTORY and frames.CYCLES == 3988:
            PENDING.append('Tinsel::_vm->_mousePos.x = 200, Tinsel::_vm->_mousePos.y = 190, Tinsel::_vm->_cursor->DoCursorMove()')
            return True
        if ARCH_INVENTORY and frames.CYCLES == 4000:
            value = frame.EvaluateExpression('Tinsel::ControlIsOn()')
            if value.GetError().Fail() or not value.GetValueAsUnsigned():
                raise RuntimeError('entry dialogue has not returned control before pockets input')
            PENDING.append('Tinsel::ActorEvent(7, (Tinsel::TINSEL_EVENT)9, (Tinsel::PLR_EVENT)16)')
            return True
        if frames.CYCLES == 450:
            PENDING.extend(['Tinsel::EndDw1Intro()', f'Tinsel::SetNewScene({ROOM}u, 1, 1)'])
            return True
        target = frame.GetThread().GetProcess().GetTarget()
        if (1108 <= frames.CYCLES <= 1130 or
                (UNIVERSITY_WALK and 1424 <= frames.CYCLES <= 1496)):
            movers = target.FindFirstGlobalVariable('Tinsel::g_Movers')
            mover = movers.GetChildAtIndex(0)
            fields = ['actorID', 'objX', 'objY', 'targetX', 'targetY',
                      'ItargetX', 'ItargetY', 'UtargetX', 'UtargetY',
                      'hCpath', 'hIpath', 'hUpath', 'bMoving', 'bHidden',
                      'bSpecReel', 'bNoPath', 'bIgPath', 'stepCount']
            state = {}
            for name in fields:
                value = mover.GetChildMemberWithName(name)
                if not value.IsValid() or value.GetError().Fail():
                    raise RuntimeError(f'mover field unavailable: {name}')
                state[name] = value.GetValueAsSigned()
            frames.EVENTS.append(dict(kind='mover_before_cycle',
                                      cycle=frames.CYCLES, state=state))
        scene = target.FindFirstGlobalVariable('Tinsel::g_SceneHandle')
        active = target.FindFirstGlobalVariable('Tinsel::g_bTagsActive')
        if frames.CYCLES > 450 and START is None:
            if frames.unsigned(scene) == ROOM and frames.unsigned(active):
                START = frames.CYCLES
        if START is not None:
            elapsed = frames.CYCLES - START
            if ARCH_SKIP and elapsed == 460:
                PENDING.append('Tinsel::PlayerEvent((Tinsel::PLR_EVENT)4, Tinsel::_vm->_mousePos)')
                return True
            if ARCH_ENTRY and elapsed == 240:
                PENDING.append('Tinsel::RunPolyTinselCode(50, (Tinsel::TINSEL_EVENT)9, (Tinsel::PLR_EVENT)16, false)')
                return True
            if UNIVERSITY_RETARGET and elapsed == 192:
                PENDING.append('Tinsel::_vm->_mousePos.x = 110, Tinsel::_vm->_mousePos.y = 180, Tinsel::_vm->_cursor->DoCursorMove()')
                return True
            if UNIVERSITY_RETARGET and elapsed == 193:
                PENDING.append('Tinsel::PlayerEvent((Tinsel::PLR_EVENT)1, Tinsel::_vm->_mousePos)')
                return True
            if UNIVERSITY_WALK and elapsed == 168:
                PENDING.append('Tinsel::_vm->_mousePos.x = 175, Tinsel::_vm->_mousePos.y = 180, Tinsel::_vm->_cursor->DoCursorMove()')
                return True
            if UNIVERSITY_WALK and elapsed == 169:
                PENDING.append('Tinsel::PlayerEvent((Tinsel::PLR_EVENT)1, Tinsel::_vm->_mousePos)')
                return True
            if DOOR_EXIT and elapsed == 72:
                PENDING.append('Tinsel::RunPolyTinselCode(6, (Tinsel::TINSEL_EVENT)9, (Tinsel::PLR_EVENT)16, false)')
                return True
            if WALK and WALK_DELAY == 1 and elapsed == 1:
                if WALK_FROM_ACTOR:
                    PENDING.append('Tinsel::_vm->_mousePos.x = 250, Tinsel::_vm->_mousePos.y = 180, Tinsel::_vm->_cursor->DoCursorMove()')
                PENDING.append('Tinsel::PlayerEvent((Tinsel::PLR_EVENT)1, Tinsel::_vm->_mousePos)')
                return True
            if elapsed % 12 == 0 and elapsed // 12 < len(POINTS):
                x, y = POINTS[elapsed // 12]
                # Set the logical driver position without the dummy SDL backend's
                # warp conversion, then run the original cursor movement logic.
                PENDING.append(f'Tinsel::_vm->_mousePos.x = {x}, '
                               f'Tinsel::_vm->_mousePos.y = {y}, '
                               'Tinsel::_vm->_cursor->DoCursorMove()')
                if WALK and WALK_DELAY == 0 and elapsed == 0:
                    PENDING.append('Tinsel::PlayerEvent((Tinsel::PLR_EVENT)1, Tinsel::_vm->_mousePos)')
                if INVENTORY and elapsed == 0:
                    # LLDB does not expose these enumerators in this build.
                    # events.h: WALKTO=9, PLR_NOEVENT=16. The resolved pockets
                    # command bypasses raw-click double-click arbitration.
                    if INVENTORY_RESIZE:
                        PENDING.append('Tinsel::_vm->_dialogs->popUpInventory(1, 0)')
                    else:
                        PENDING.append('Tinsel::ActorEvent(7, (Tinsel::TINSEL_EVENT)9, (Tinsel::PLR_EVENT)16)')
                if DOOR_ACTION and elapsed == 0:
                    PENDING.append('Tinsel::RunPolyTinselCode(6, (Tinsel::TINSEL_EVENT)11, (Tinsel::PLR_EVENT)16, false)')
                if LOOK and elapsed == 0:
                    PENDING.append('Tinsel::ActorEvent(7, (Tinsel::TINSEL_EVENT)10, (Tinsel::PLR_EVENT)16)')
                if INVENTORY_DRAG and elapsed == 12:
                    PENDING.extend(['Tinsel::PlayerEvent((Tinsel::PLR_EVENT)11, Tinsel::_vm->_mousePos)',
                                    'Tinsel::PlayerEvent((Tinsel::PLR_EVENT)1, Tinsel::_vm->_mousePos)'])
                if INVENTORY_DRAG and elapsed == 36:
                    PENDING.append('Tinsel::PlayerEvent((Tinsel::PLR_EVENT)12, Tinsel::_vm->_mousePos)')
                if INVENTORY and elapsed == (48 if INVENTORY_DRAG else 36):
                    PENDING.append('Tinsel::_vm->_dialogs->closeInventory()')
                return True
        if frames.CYCLES >= (4100 if ARCH_INVENTORY else 4000):
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
    if ARCH_INVENTORY and len(frames.RECORDS) >= 480 and frames.CYCLES < 4000:
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
        if INVENTORY or ARCH_INVENTORY:
            dialogs = vm.Dereference().GetChildMemberWithName('_dialogs').Dereference()
            frames.RECORDS[-1]['inventory'] = [frames.unsigned(dialogs.GetChildMemberWithName(name))
                                               for name in ['_inventoryState', '_activeInv']]
            frames.RECORDS[-1]['inventory_dragging'] = frames.unsigned(dialogs.GetChildMemberWithName('_invDragging'))
        if LOOK:
            try:
                mixer_type = target.FindFirstType('Audio::MixerImpl')
                mixer = vm.Dereference().GetChildMemberWithName('_mixer').Cast(
                    mixer_type.GetPointerType()).Dereference()
                channels = mixer.GetChildMemberWithName('_channels')
                if channels.GetNumChildren() != 32:
                    raise RuntimeError('reference mixer channels unavailable')
                speech = []
                for index in range(32):
                    pointer = channels.GetChildAtIndex(index)
                    if frames.unsigned(pointer) == 0:
                        continue
                    channel = pointer.Dereference()
                    if frames.unsigned(channel.GetChildMemberWithName('_type')) == 3:
                        speech.append({name: frames.unsigned(channel.GetChildMemberWithName(name))
                                       for name in ['_samplesConsumed', '_samplesDecoded', '_mixerTimeStamp']})
                frames.RECORDS[-1]['mixer_speech'] = speech
                frames.RECORDS[-1]['mixer_rate'] = frames.unsigned(mixer.GetChildMemberWithName('_sampleRate'))
            except Exception as error:
                frames.FAILURES.append(str(error))
                return True
        for name in ['g_TaggedActor', 'g_hTaggedPolygon']:
            value = target.FindFirstGlobalVariable('Tinsel::' + name)
            if value.IsValid():
                frames.RECORDS[-1][name] = value.GetValueAsSigned()
    return result


def palette(frame, location, dictionary):
    return frames.palette(frame, location, dictionary)


def sample_pcm(frame, location, dictionary):
    return frames.sample_pcm(frame, location, dictionary)


def samples_stop(frame, location, dictionary):
    return frames.samples_stop(frame, location, dictionary)


def cursor_order(frame, location, dictionary):
    """Read process order and raw object state; never execute target code."""
    if START is None or not START + 72 <= frames.CYCLES <= START + 90:
        return False
    try:
        target = frame.GetThread().GetProcess().GetTarget()
        vm = target.FindFirstGlobalVariable('Tinsel::_vm').Dereference()
        mouse = vm.GetChildMemberWithName('_mousePos')
        cursor = vm.GetChildMemberWithName('_cursor').Dereference()
        if not all(value.IsValid() for value in [vm, mouse, cursor]):
            raise RuntimeError('cursor-order state unavailable')
        main = cursor.GetChildMemberWithName('_mainCursor')
        event = {'cycle': frames.CYCLES, 'kind': 'cursor_process_order',
                 'function': frame.GetFunctionName(),
                 'driver_cursor': [mouse.GetChildMemberWithName(axis).GetValueAsSigned()
                                   for axis in ['x', 'y']],
                 'callers': callers(frame)}
        if not main.IsValid():
            raise RuntimeError('cursor-order main object unavailable')
        if frames.unsigned(main):
            obj = main.Dereference()
            if not all(obj.GetChildMemberWithName(name).IsValid()
                       for name in ['xPos', 'yPos', 'hImg']):
                raise RuntimeError('cursor-order object fields unavailable')
            # These are raw fixed-point object origins, not animation anchors.
            event['object_origin_fixed'] = [obj.GetChildMemberWithName(axis).GetValueAsSigned()
                                            for axis in ['xPos', 'yPos']]
            event['image'] = frames.unsigned(obj.GetChildMemberWithName('hImg'))
        frames.EVENTS.append(event)
        return False
    except Exception as error:
        frames.FAILURES.append(str(error))
        return True


def print_tag(frame, location, dictionary):
    try:
        frames.EVENTS.append({'cycle': frames.CYCLES, 'kind': 'print_tag',
                              'polygon': frame.FindVariable('hp').GetValueAsSigned(),
                              'text': frames.unsigned(frame.FindVariable('text'))})
        return False
    except Exception as error:
        frames.FAILURES.append(str(error))
        return True


def animation_init(frame, location, dictionary):
    if frames.CYCLES < 450:
        return False
    result = frames.animation(frame, location, dictionary)
    if not result and (1108 <= frames.CYCLES <= 1130 or
                       (DOOR_EXIT and 1358 <= frames.CYCLES <= 1366)):
        frames.EVENTS[-1]['callers'] = callers(frame)
    return result


def callers(frame):
    """Read call sites without executing target expressions or changing state."""
    thread = frame.GetThread()
    return [thread.GetFrameAtIndex(i).GetFunctionName()
            for i in range(min(thread.GetNumFrames(), 10))]


def animation_step(frame, location, dictionary):
    if START is None and not (450 <= frames.CYCLES <= 455 or 1108 <= frames.CYCLES <= 1130):
        return False
    try:
        state = frame.FindVariable('pAnim').Dereference()
        frames.EVENTS.append({
            'kind': 'animation_step', 'cycle': frames.CYCLES,
            'address': state.GetLoadAddress(),
            'script': frames.unsigned(state.GetChildMemberWithName('hScript')),
            'index': frames.unsigned(state.GetChildMemberWithName('scriptIndex')),
            'period': frames.unsigned(state.GetChildMemberWithName('aniRate')),
            'remaining': frames.unsigned(state.GetChildMemberWithName('aniDelta')),
        })
        if (1108 <= frames.CYCLES <= 1130 or
                (DOOR_EXIT and 1358 <= frames.CYCLES <= 1366)):
            frames.EVENTS[-1]['callers'] = callers(frame)
        return False
    except Exception as error:
        frames.FAILURES.append(str(error))
        return True


def backdrop_before(frame, location, dictionary):
    global BACKDROP_PENDING
    if not 4000 <= frames.CYCLES <= 4023:
        return False
    try:
        if BACKDROP_PENDING is not None:
            raise RuntimeError('nested translucent rectangle capture')
        target = frame.GetThread().GetProcess().GetTarget()
        process = frame.GetThread().GetProcess()
        surface = target.FindFirstGlobalVariable('Tinsel::_vm').Dereference().GetChildMemberWithName('_screenSurface')
        base = frames.unsigned(surface.GetChildMemberWithName('pixels'))
        pitch = frames.unsigned(surface.GetChildMemberWithName('pitch'))
        if pitch != 320:
            raise RuntimeError('unexpected backdrop pitch')
        obj = frame.FindVariable('pObj').Dereference()
        width = obj.GetChildMemberWithName('width').GetValueAsSigned()
        height = obj.GetChildMemberWithName('height').GetValueAsSigned()
        if frames.unsigned(frame.FindVariable('applyClipping')):
            width -= sum(obj.GetChildMemberWithName(n).GetValueAsSigned() for n in ['leftClip', 'rightClip'])
            height -= sum(obj.GetChildMemberWithName(n).GetValueAsSigned() for n in ['topClip', 'botClip'])
        offset = frames.unsigned(frame.FindVariable('destP')) - base
        x, y = offset % 320, offset // 320
        if not (0 < width <= 320 - x and 0 < height <= 200 - y and y >= 0):
            raise RuntimeError(f'unexpected backdrop bounds: {x,y,width,height}')
        table = target.FindFirstGlobalVariable('Tinsel::g_transPalette')
        lookup = frames.read(process, table.GetLoadAddress(), 256)
        before = frames.read(process, base, 64000)
        stem = f'backdrop-{len(BACKDROPS):04d}'
        (frames.OUTPUT / (stem + '.before.idx')).write_bytes(before)
        (frames.OUTPUT / (stem + '.table')).write_bytes(lookup)
        (frames.OUTPUT / (stem + '.bounds')).write_bytes(struct.pack('<4i', x, y, width, height))
        BACKDROP_PENDING = dict(stem=stem, cycle=frames.CYCLES, bounds=[x,y,width,height], base=base,
                               before=before, lookup=lookup)
        return False
    except Exception as error:
        frames.FAILURES.append(str(error))
        return True


def backdrop_after(frame, location, dictionary):
    global BACKDROP_PENDING
    if BACKDROP_PENDING is None:
        return False
    try:
        record = BACKDROP_PENDING
        after = frames.read(frame.GetThread().GetProcess(), record.pop('base'), 64000)
        before, lookup = record.pop('before'), record.pop('lookup')
        expected = bytearray(before)
        x,y,width,height = record['bounds']
        for yy in range(y,y+height):
            for xx in range(x,x+width):
                at = yy*320+xx
                expected[at] = lookup[before[at]]
        (frames.OUTPUT / (record['stem'] + '.after.idx')).write_bytes(after)
        record.update(before_sha256=hashlib.sha256(before).hexdigest(), after_sha256=hashlib.sha256(after).hexdigest(),
                      table_sha256=hashlib.sha256(lookup).hexdigest(), mapped_pixels=sum(a!=b for a,b in zip(before,after)),
                      lookup_exact=after==expected)
        BACKDROPS.append(record)
        BACKDROP_PENDING = None
        if not record['lookup_exact']:
            raise RuntimeError('original backdrop differs from observed lookup mapping')
        return False
    except Exception as error:
        frames.FAILURES.append(str(error))
        return True


def finish(debugger):
    frames.finish(debugger)
    path = frames.OUTPUT / 'capture.json'
    report = json.loads(path.read_text())
    report['backdrops'] = BACKDROPS
    if BACKDROP_PENDING is not None:
        report['errors'].append('unfinished backdrop capture')
        report['complete'] = False
    report['cursor_order_diagnostic'] = TRACE_CURSOR_ORDER
    report.update(scope='Controlled original debug scene entry, logical driver writes and original DoCursorMove; not normal host input or full gameplay equivalence.',
                  arch_inventory=ARCH_INVENTORY, injected_actions=ACTIONS, hover_start_cycle=START, inventory_diagnostic=INVENTORY, arch_skip=ARCH_SKIP, arch_entry=ARCH_ENTRY, university_retarget=UNIVERSITY_RETARGET, university_walk=UNIVERSITY_WALK, inventory_drag=INVENTORY_DRAG, door_exit=DOOR_EXIT, door_action=DOOR_ACTION, resize_delta=RESIZE_DELTA, resize_corner=RESIZE_CORNER, inventory_resize=INVENTORY_RESIZE, inventory_fixed_corner=INVENTORY_FIXED_CORNER, look_diagnostic=LOOK, walk_diagnostic=WALK, walk_delay=WALK_DELAY, walk_from_actor=WALK_FROM_ACTOR)
    path.write_text(json.dumps(report, indent=2) + '\n')


def __lldb_init_module(debugger, dictionary):
    if frames.BACKEND != 'surfacesdl' or not 72 <= frames.LIMIT <= 504:
        raise ValueError('hover probe requires surfacesdl and 72..504 frames')
    frames.OUTPUT.mkdir()
    target = debugger.GetSelectedTarget()
    points = [
        (target.BreakpointCreateByName('SurfaceSdlGraphicsManager::setPalette'), __name__ + '.palette'),
        (target.BreakpointCreateByLocation('background.cpp', 288), __name__ + '.capture'),
        (target.BreakpointCreateByName('Tinsel::TinselEngine::NextGameCycle'), __name__ + '.cycle'),
        (target.BreakpointCreateByName('Tinsel::PrintTag'), __name__ + '.print_tag'),
        (target.BreakpointCreateByName('Tinsel::StepAnimScript'), __name__ + '.animation_step'),
        (target.BreakpointCreateByName('Tinsel::InitStepAnimScript'), __name__ + '.animation_init'),
        (target.BreakpointCreateByLocation('sound.cpp', 165), __name__ + '.sample_pcm'),
        (target.BreakpointCreateByName('Tinsel::SoundManager::stopAllSamples'), __name__ + '.samples_stop'),
    ]
    if TRACE_CURSOR_ORDER:
        points.extend((target.BreakpointCreateByName(name), __name__ + '.cursor_order')
                      for name in ['Tinsel::PointProcess', 'Tinsel::TagProcess',
                                   'Tinsel::Cursor::DoCursorMove'])
    if TRACE_BACKDROP:
        points.extend([(target.BreakpointCreateByName('Tinsel::WrtTrans'), __name__ + '.backdrop_before'),
                       (target.BreakpointCreateByLocation('graphics.cpp', 1262), __name__ + '.backdrop_after')])
    for bp, callback in points:
        if bp.GetNumLocations() != 1:
            raise RuntimeError(f'nonunique hover breakpoint: {callback}')
        bp.SetScriptCallbackFunction(callback)
