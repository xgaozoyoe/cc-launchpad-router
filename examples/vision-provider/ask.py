#!/usr/bin/env python3
"""Send one local image to the private vision API. Never print its API key."""
import argparse
import base64
import json
from pathlib import Path
from urllib.request import Request, urlopen
from urllib.error import HTTPError


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('image', type=Path)
    parser.add_argument('prompt', nargs='?', default='Describe this image.')
    parser.add_argument('--port', type=int, default=8002)
    parser.add_argument('--max-tokens', type=int, default=512)
    args = parser.parse_args()
    if not 16 <= args.max_tokens <= 4096:
        parser.error('--max-tokens must be between 16 and 4096')
    image = args.image.read_bytes()
    if len(image) > 4 * 1024 * 1024:
        parser.error('Use a PNG or JPEG no larger than 4 MiB')
    if image.startswith(b'\x89PNG\r\n\x1a\n'):
        mime = 'image/png'
    elif image.startswith(b'\xff\xd8\xff'):
        mime = 'image/jpeg'
    else:
        parser.error('Only PNG and JPEG images are supported')
    config = dict(line.split('=', 1) for line in (Path(__file__).parent / '.env').read_text().splitlines() if '=' in line)
    body = {'model': 'qwen2.5-vl-3b-instruct', 'messages': [{'role': 'user', 'content': [
        {'type': 'image_url', 'image_url': {'url': 'data:'+mime+';base64,'+base64.b64encode(image).decode()}},
        {'type': 'text', 'text': args.prompt}]}], 'max_tokens': args.max_tokens, 'temperature': 0, 'stream': False}
    request = Request(f'http://127.0.0.1:{args.port}/v1/chat/completions', data=json.dumps(body).encode(),
                      headers={'Content-Type': 'application/json', 'Authorization': 'Bearer '+config['INFERENCE_API_KEY']})
    try:
        with urlopen(request, timeout=180) as response:
            result = json.load(response)
    except HTTPError as error:
        raise SystemExit(f'API HTTP {error.code}: '+error.read(4096).decode(errors='replace'))
    print(result['choices'][0]['message']['content'])
    print(json.dumps({'finish_reason': result['choices'][0]['finish_reason'], 'usage': result['usage']}))

if __name__ == '__main__':
    main()
