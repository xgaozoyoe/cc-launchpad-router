# Version pinned for a reproducible Qwen3 demo; review before production use.
FROM vllm/vllm-openai:v0.10.2
COPY scripts/serve.py /opt/launchpad/serve.py
COPY scripts/healthcheck.py /opt/launchpad/healthcheck.py
EXPOSE 8000
ENTRYPOINT ["python3", "/opt/launchpad/serve.py"]
