"""Verify auth, model discovery, real generation, and streaming usage. No dependencies."""
import argparse
import json
import os
from pathlib import Path
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--base-url', default='http://127.0.0.1:8000/v1')
    args = parser.parse_args()
    base = args.base_url.rstrip('/')
    key = os.environ.get('INFERENCE_API_KEY')
    if not key:
        envfile = Path(__file__).resolve().parents[1] / '.env'
        if envfile.exists():
            key = next((line.partition('=')[2].strip() for line in envfile.read_text().splitlines()
                        if line.startswith('INFERENCE_API_KEY=')), None)
    if not key:
        raise ValueError('Set INFERENCE_API_KEY or run scripts/setup.py.')
    if not base.startswith(('http://127.0.0.1:', 'http://localhost:', 'https://')):
        raise ValueError('Use HTTPS for remote endpoints.')

    def request(path, payload=None, auth=True):
        headers = {'Content-Type': 'application/json'}
        if auth:
            headers['Authorization'] = 'Bearer ' + key
        return urllib.request.urlopen(urllib.request.Request(
            base + path, data=json.dumps(payload).encode() if payload else None,
            headers=headers), timeout=180)

    try:
        with request('/models', auth=False):
            pass
    except urllib.error.HTTPError as error:
        if error.code not in (401, 403):
            raise ValueError('Unexpected unauthenticated status') from None
    else:
        raise ValueError('Endpoint allows unauthenticated model access')
    with request('/models') as response:
        models = json.load(response)
    model = 'qwen3-8b-awq'
    if model not in [item['id'] for item in models['data']]:
        raise ValueError('Expected served model is missing')
    payload = {'model': model, 'messages': [{'role': 'user', 'content': '用一句话介绍千问。'}],
               'max_tokens': 128, 'temperature': 0.7, 'top_p': 0.8,
               'chat_template_kwargs': {'enable_thinking': False}}
    with request('/chat/completions', payload) as response:
        result = json.load(response)
    if not result['choices'][0]['message']['content']:
        raise ValueError('Empty completion')
    usage = result['usage']
    if not usage['prompt_tokens'] > 0 or not usage['completion_tokens'] > 0:
        raise ValueError('Missing token usage')
    print('PASS: authentication, model discovery, non-streaming generation and usage')
    print('Answer:', result['choices'][0]['message']['content'])
    print('Usage:', json.dumps(usage))
    payload.update(stream=True, stream_options={'include_usage': True})
    done, tokens, text = False, 0, ''
    with request('/chat/completions', payload) as response:
        for line in response:
            line = line.decode().strip()
            if not line.startswith('data: '):
                continue
            if line == 'data: [DONE]':
                done = True
                break
            chunk = json.loads(line[6:])
            for choice in chunk.get('choices', []):
                text += choice.get('delta', {}).get('content') or ''
            if chunk.get('usage'):
                tokens = chunk['usage']['completion_tokens']
    if not done or not text or tokens <= 0:
        raise ValueError('Incomplete stream or missing final usage')
    print('PASS: streaming generation, final usage and [DONE]')


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        # Do not leak headers, request bodies, or upstream error pages.
        raise SystemExit('FAIL: ' + type(error).__name__ + '; check endpoint readiness/configuration.')
