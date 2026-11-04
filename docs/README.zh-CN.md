# 通过平台 FRPS 发布千问推理 API

**所有算力供应商必须运行 FRPC，连接平台统一的 FRPS 服务器。** 供应商不自行对外发布域名或直连推理接口。

平台使用一个 HTTPS 域名，每次 credit 发行都有独立的 launch ID：

```text
https://api.example.com/launches/<launch-id>/v1
POST /launches/<launch-id>/v1/chat/completions
```

Provider ID 标识供应商隧道，launch ID 标识一次 credit 发行。一个供应商可承载多个 launch，但各 launch 的额度和余额独立，不能因为共用隧道而合并。

## 供应商流程

1. 向平台领取自己的 FRPC 配置包：`frpc.toml`、`ca.crt`、`token`。
2. 在支持 NVIDIA GPU 的 Linux 机器上克隆仓库，执行 `python3 scripts/setup.py`。
3. 将配置包放入 `.secrets/frp-provider/`，限制文件权限。
4. 启动千问模型和 FRPC：

```bash
docker compose -f compose.yaml -f compose.frpc.yaml up --build -d
```

5. 模型加载后测试本机推理，再使用平台分配的 launch 地址测试：

```bash
python3 scripts/smoke.py
python3 scripts/smoke.py --base-url https://api.example.com/launches/launch-001/v1
```

示例域名和 launch ID 需要替换为真实值。推理 API key 与 FRP 连接凭证是两类密钥，均不可提交 Git 或发给买家。

完整说明：[供应商部署](HOSTING.md)、[平台 FRPS 与 launch 注册](FRP-HOSTING.md)。

目前路由器是传输示例，测试时透传供应商 Bearer key。正式买家接入还需要平台身份验证、按 launch ID 检查和预留 credits、服务端密钥管理、计量和 burn；这些功能尚未实现，网页也没有自动同步这里的 launch 注册表。已验证真实本地 FRP 隧道和模拟模型的端到端路由，尚未验证 GPU 实机推理及公网部署。
