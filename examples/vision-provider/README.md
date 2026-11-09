# Vision provider demo

Qwen2.5-VL-3B-Instruct accepts text and a single image. This separate deployment does not replace the Qwen3 text provider or create a credit offer.

Requirements: Linux, Docker Compose, NVIDIA Container Toolkit, an available NVIDIA GPU and enough disk space for the model and image. The tested target is one RTX 4090. Model revision and inference image are pinned in `compose.yaml`.

```sh
./run.sh --gpu 2 --port 8002
docker compose logs -f inference
python3 ask.py /path/to/photo.jpg 'What is in this image?'
```

The API listens on localhost only. `run.sh` creates an API credential in `.env` on first use; do not commit it. Run commands from this directory. To stop: `docker compose stop`. To restart on a different GPU/port, run `run.sh` again with both arguments. Docker restarts the container automatically after a host reboot.

## Serving limits

- Total context: 8,192 tokens, including image tokens, text and output.
- Example client output: 16–4,096 tokens, subject to remaining context.
- One image per request; no video. The example client accepts PNG/JPEG up to 4 MiB.
- Image preprocessing is capped at 1,003,520 pixels; very small text and large documents may need cropping.
- FP16 weights (this model is not AWQ quantized).

The client sends an OpenAI-compatible `messages` array with `image_url` (inline base64 data) and `text` content parts to `/v1/chat/completions`. A text-only Qwen3 launch cannot accept this request. Platform image uploads, vision launch registration and image token pricing must be explicitly supported before selling this model's credits.

Official model: https://huggingface.co/Qwen/Qwen2.5-VL-3B-Instruct

## Smoke-test findings (2026-11-09)

The deployed demo rejected missing/incorrect keys and correctly identified a generated blue image. Actual usage was 101 input tokens and 2 output tokens. The same multimodal messages sent to `/tokenize` reported only 38 tokens: this tokenizer endpoint does **not** provide a reliable image-inclusive context count in this configuration. Do not reuse the platform's text-only paid context preflight for this model. Vision launch/UI integration remains pending an image-aware context check and explicit limits; this demo is not listed for credit purchasing.
