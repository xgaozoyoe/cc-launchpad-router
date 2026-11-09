#!/usr/bin/env python3
"""Local image-understanding/authentication smoke test. No platform credit usage."""
import base64
import json
import struct
import zlib
from pathlib import Path
from urllib.request import Request, urlopen
from urllib.error import HTTPError


def main():
    config = dict(line.split('=', 1) for line in (Path(__file__).parent / '.env').read_text().splitlines() if '=' in line)
    key = config['INFERENCE_API_KEY']
    base = 'http://127.0.0.1:'+config.get('PORT', '8002')
    def call(path, body=None, credential=key):
        headers = {'Content-Type': 'application/json'}
        if credential is not None: headers['Authorization'] = 'Bearer '+credential
        with urlopen(Request(base+path, data=None if body is None else json.dumps(body).encode(), headers=headers), timeout=180) as r:
            return json.load(r)
    for credential in [None, 'incorrect-test-key']:
        try:
            call('/v1/models', credential=credential)
            raise AssertionError('Unauthorized access accepted')
        except HTTPError as e:
            assert e.code in (401, 403), e.code
    print('PASS: missing/wrong API keys rejected')
    model = 'qwen2.5-vl-3b-instruct'
    assert any(m['id'] == model for m in call('/v1/models')['data'])
    def chunk(kind, data):
        return struct.pack('>I', len(data))+kind+data+struct.pack('>I', zlib.crc32(kind+data))
    # Solid blue image generated locally; only the image carries the tested color.
    png = b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR', struct.pack('>IIBBBBB', 224,224,8,2,0,0,0))
    png += chunk(b'IDAT', zlib.compress((b'\0'+b'\0\0\xff'*224)*224))+chunk(b'IEND', b'')
    messages = [{'role':'user','content':[
        {'type':'image_url','image_url':{'url':'data:image/png;base64,'+base64.b64encode(png).decode()}},
        {'type':'text','text':'What is the dominant color in this image? Answer with only the color name.'}]}]
    result = call('/v1/chat/completions', {'model':model,'messages':messages,'temperature':0,'max_tokens':128})
    answer = result['choices'][0]['message']['content']
    assert 'blue' in answer.lower(), answer
    assert result['choices'][0]['finish_reason'] == 'stop'
    usage = result['usage']
    assert usage['prompt_tokens'] > 0 and usage['completion_tokens'] > 0
    assert usage['total_tokens'] == usage['prompt_tokens']+usage['completion_tokens']
    print('PASS: actual image recognized:', answer)
    print('Usage:', json.dumps(usage))
    # Inspect whether this vLLM version includes image tokens in its tokenize API.
    try:
        tokenized = call('/tokenize', {'model':model,'messages':messages,'add_generation_prompt':True})
        print('Tokenizer count:', tokenized.get('count'), 'generation input:', usage['prompt_tokens'])
    except HTTPError as e:
        print('Multimodal tokenize HTTP:', e.code)

if __name__ == '__main__':
    main()
