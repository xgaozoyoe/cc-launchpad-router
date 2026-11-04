"""Fail closed before starting the GPU inference server."""
import os
import sys


def server_args(env):
    key = env.get('INFERENCE_API_KEY', '')
    if len(key) < 32 or any(c.isspace() for c in key):
        raise ValueError('INFERENCE_API_KEY must contain at least 32 non-whitespace characters.')
    context = int(env.get('MAX_MODEL_LEN', '4096'))
    concurrency = int(env.get('MAX_NUM_SEQS', '2'))
    memory = float(env.get('GPU_MEMORY_UTILIZATION', '0.85'))
    if not 512 <= context <= 32768 or not 1 <= concurrency <= 32 or not 0.1 <= memory <= 0.95:
        raise ValueError('Invalid context, concurrency, or GPU memory setting.')
    return [sys.executable, '-m', 'vllm.entrypoints.openai.api_server',
            '--host', '0.0.0.0', '--port', '8000',
            '--model', env.get('MODEL_ID', 'Qwen/Qwen3-8B-AWQ'),
            '--revision', env.get('MODEL_REVISION', '93eb32ea9ef98cc266b85563ad0dd29e5f2940b9'),
            '--served-model-name', env.get('SERVED_MODEL_NAME', 'qwen3-8b-awq'),
            '--dtype', 'half', '--quantization', 'awq',
            '--max-model-len', str(context), '--max-num-seqs', str(concurrency),
            '--gpu-memory-utilization', str(memory), '--enforce-eager',
            '--disable-log-requests', '--api-key', key]


if __name__ == '__main__':
    try:
        args = server_args(os.environ)
    except ValueError as error:
        raise SystemExit(str(error))
    os.execv(sys.executable, args)
