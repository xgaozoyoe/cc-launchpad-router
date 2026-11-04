# Qwen Supplier Starter — 千问量化推理服务

**Start here: [From a local model to a hosted REST API](HOSTING.md).**

Includes local inference, an optional HTTPS reverse proxy, and a named-tunnel configuration for home GPUs. GitHub hosts this source; your NVIDIA GPU machine runs the model.


在供应商自己的 GPU 机器上启动真实推理服务，供 AI Computing Credit Launchpad 后续接入。
默认模型为官方 **Qwen/Qwen3-8B-AWQ，4-bit AWQ**，以 `qwen3-8b-awq` 对外服务。
本目录不依赖网站的 Google 登录，也不包含任何 Google 密钥。

## 运行环境

- Linux x86_64 + NVIDIA GPU，建议从 Ampere 架构及以后、16GB 或以上显存开始试运行。
  这是保守的 demo 配置建议，不是经过本项目实机验证的最低要求；实际用量取决于 GPU、上下文和并发。
- 安装 NVIDIA 驱动、Docker Engine、Docker Compose v2、NVIDIA Container Toolkit，并为 Docker 配置 NVIDIA runtime。
- 建议主机内存至少 16GB、可用磁盘至少 30GB；首次运行需要联网下载镜像和模型，可能较久。
- Apple Silicon Mac 的 Docker 无法直接使用 Apple GPU，本配置也不支持 AMD GPU。
- Windows 可在配置好 NVIDIA GPU 透传的 WSL2 Linux 环境中尝试；本项目未验证。

先执行 `nvidia-smi`，再按 NVIDIA 文档确认容器 GPU 可用：
https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html

## 三步启动

在此目录执行：

```bash
python3 scripts/setup.py
docker compose up --build -d
docker compose logs -f inference
```

setup 会生成随机 API key，保存到仅当前用户可读的 `.env`；不会打印或覆盖现有配置。
看到模型加载完成后，另开终端执行：

```bash
docker compose ps
python3 scripts/smoke.py
```

验收脚本检查：无 key 被拒绝、模型列表、中文真实推理、输入/输出 token 用量、SSE 流式回答和最后的 usage。
需要 Python 3，无额外 pip 依赖。它会发起两次真实推理，不是固定 mock 回答。

停止：`docker compose down`。模型保存在命名 volume，下次启动复用；加 `-v` 会删除模型缓存。

## API 合约

Base URL：`http://127.0.0.1:8000/v1`

| 路径 | 用途 |
| --- | --- |
| `GET /health`（不带 /v1） | 服务健康；仅用于运行监控，不证明模型质量或算力所有权 |
| `GET /v1/models` | 查询模型，需 Bearer key |
| `POST /v1/chat/completions` | 聊天推理，需 Bearer key |

请求示例（Authorization header 中使用本机 `.env` 内的 INFERENCE_API_KEY）：

```json
{
  "model": "qwen3-8b-awq",
  "messages": [{"role": "user", "content": "请解释什么是计算信用。"}],
  "max_tokens": 256,
  "temperature": 0.7,
  "top_p": 0.8,
  "chat_template_kwargs": {"enable_thinking": false},
  "stream": false
}
```

返回使用标准 `choices[0].message.content`，并包含 `usage.prompt_tokens`、
`usage.completion_tokens` 和 `usage.total_tokens`。chat template 由模型服务处理。
`enable_thinking` 是 Qwen/vLLM 扩展参数；demo 请求禁用 thinking，减少演示延迟。
若允许 thinking，后续计费合约必须明确是否计入 reasoning tokens。

流式请求设置 `stream: true` 并添加 `stream_options: {"include_usage": true}`；
客户端读取 `choices[].delta.content`，末尾 usage 和 `data: [DONE]`。

默认上下文上限 4096 tokens（输入与输出合计）、并发 2、单 GPU；可在 `.env` 修改。
启动采用 eager 模式降低 CUDA Graph 的额外显存需求，牺牲部分性能。
显存不足先降低 MAX_MODEL_LEN/MAX_NUM_SEQS；不要把量化权重大小当作总显存需求。

## 接入 Launchpad 的方式

```text
买家 → Launchpad 服务端网关 → 此 Docker 推理接口
         │                       │
         ├─核对并预留 credits     └─真实回答与 usage
         └─核验用量、记录请求、扣除 / burn credits
```

当前网站仍是模拟推理，本包没有自动注册供应商、修改模型清单、扣除 credits、
链上 burn 或质押 PRL。接口验收也不等于算力所有权证明。

后续供应商注册应记录：

- Base URL、服务端保存的 API key、供应商 ID。
- 精确模型仓库、revision、AWQ 4-bit、tokenizer、4096 上下文与容量承诺。
- credit launch 与该服务绑定；不同模型、精度或版本不能自动视为相同资产。

默认仅监听宿主机 127.0.0.1。云端 Launchpad 无法访问供应商机器的 localhost。
远程接入须提供平台可达的 HTTPS 域名，通过反向代理转发到本机 8000；
将 key 单独交给平台服务端，不能放入 React、浏览器 localStorage 或公开配置。
若代理运行在另一容器，需配置私有容器网络；跨主机则使用私有网络或明确设置 BIND_ADDRESS。
不要直接把无 TLS 的 8000 端口公开到互联网。健康接口同样应受网络访问控制。

PRL 质押、容量基准测试、请求审计、平台侧用量核验、失败重试去重、池兜底由平台负责，
不在此模型容器内完成。

## 版本与验证范围

- 镜像：`vllm/vllm-openai:v0.10.2`，固定兼容版本，非 latest；正式发布前应审查更新并锁定镜像 digest。
- 模型 revision：`93eb32ea9ef98cc266b85563ad0dd29e5f2940b9`；模型按固定 revision 下载。
- 无需 `trust-remote-code`，不把模型或密钥打入镜像。
- 本项目已检查 YAML 语法和 GPU 声明、Python 启动参数，并用内存中的模拟 HTTP/SSE 响应验证验收脚本逻辑。当前环境缺少 Compose 插件且无法访问 Docker daemon，未运行 `docker compose config`、镜像构建或 NVIDIA GPU 真实生成测试。

参考：
- Qwen 官方部署与非思考模式：https://qwen.readthedocs.io/en/latest/deployment/vllm.html
- 官方量化模型：https://huggingface.co/Qwen/Qwen3-8B-AWQ
- vLLM Docker：https://docs.vllm.ai/en/v0.10.2/deployment/docker.html
