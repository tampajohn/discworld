#!/usr/bin/env python3
"""Check actual Safari simulator loading/rendering via an existing WebDriver session.

Requires serve-browser-fixtures.py. Uses synthetic File inputs and form submission;
this deliberately does not claim file-picker, trusted touch or autoplay acceptance.
"""
import argparse
import base64
import json
from pathlib import Path
import time
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('session', help='existing Safari WebDriver session ID')
parser.add_argument('expected_png', type=Path)
parser.add_argument('report', type=Path)
parser.add_argument('--driver', default='http://127.0.0.1:8759')
parser.add_argument('--url', default='http://127.0.0.1:8767/')
args = parser.parse_args()
base = args.driver.rstrip('/') + '/session/' + args.session

def post(path, body):
    request = urllib.request.Request(base + path, data=json.dumps(body).encode(), headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=45) as response:
        result = json.load(response)['value']
    if isinstance(result, dict) and 'error' in result:
        raise RuntimeError(result)
    return result

def js(script, values=None, asynchronous=False):
    return post('/execute/async' if asynchronous else '/execute/sync', {'script': script, 'args': values or []})

post('/url', {'url': args.url})
js("""const done=arguments[arguments.length-1];
(async()=>{for(const [id,url,name] of [['iso','iso','discworld.iso'],['font','font','music.sf2'],['initial-save','save','checkpoint.dws']]) {
 const response=await fetch('/__fixtures/'+url); if(!response.ok)throw Error('fixture fetch failed');
 const files=new DataTransfer();files.items.add(new File([await response.blob()],name));document.getElementById(id).files=files.files;
}document.getElementById('open-game').requestSubmit();return true;})().then(done,e=>done({error:String(e)}));""", asynchronous=True)
deadline = time.monotonic() + 60
while True:
    state = js("return {ready:!document.querySelector('#player').hidden,failed:document.querySelector('#status').dataset.error,status:document.querySelector('#status').textContent};")
    if state['ready']:
        break
    if state['failed'] == 'true' or time.monotonic() > deadline:
        raise RuntimeError(state)
    time.sleep(.2)
encoded = base64.b64encode(args.expected_png.read_bytes()).decode()
report = js("""const done=arguments[arguments.length-1],encoded=arguments[0];
(async()=>{const image=await createImageBitmap(await(await fetch('data:image/png;base64,'+encoded)).blob());
const expected=document.createElement('canvas');expected.width=320;expected.height=200;expected.getContext('2d').drawImage(image,0,0);image.close();
const canvas=document.querySelector('#game'),actual=canvas.getContext('2d').getImageData(0,0,320,200).data;
const pixels=expected.getContext('2d').getImageData(0,0,320,200).data;
const digest=await crypto.subtle.digest('SHA-256',actual);
return {tick:canvas.dataset.tick,scene:canvas.dataset.scene,native_rgba_equal:actual.every((v,i)=>v===pixels[i]),
sha256:Array.from(new Uint8Array(digest)).map(v=>v.toString(16).padStart(2,'0')).join(''),
viewport:[innerWidth,innerHeight],user_agent:navigator.userAgent};})().then(done,e=>done({error:String(e)}));""", [encoded], asynchronous=True)
assert report['native_rgba_equal'], report
report.update(file_inputs='synthetic local fixtures', submission='scripted requestSubmit', trusted_touch_verified=False, audio_unlock_verified=False, physical_device=False)
args.report.write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps(report, indent=2))
