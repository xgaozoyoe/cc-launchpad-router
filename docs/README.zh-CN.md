# AI Computing Credit Launchpad：实现与部署说明

当前统一入口为 https://riddle.xyz。完整功能边界见 [实现状态](IMPLEMENTATION-STATUS.md)。

已接通 Google 登录、Rust + MongoDB 项目保存、钱包签名绑定、Sepolia tPRL 质押及 finalized 事件监听。确认质押后分配 Launch ID，Provider ID 为完整 stake 交易哈希。My launches 可查看本人项目、状态、交易、Launch ID、质押余额与 RPC 截止时间。

24 小时从链上 stake 区块时间开始，而非平台确认时间。已接通 FRPC 凭证、实际 RPC 检查、管理员钱包审批和链上验收。独立的测试 credit 合约支持 Sepolia ETH 购买、按项目显示余额、实际推理、批量 burn 及未使用 token 提取。PRL 钱包和公共池等概念模拟仍隔离在 Demo workspace；现有 PRL 质押保持锁定。测试币 tPRL 不是主网 PRL。

## 供应商部署目标

所有算力供应商必须运行 FRPC，连接平台 FRPS。统一路由设计为：

```text
https://api.example.com/launches/<launch-id>/v1
```

该域名是示例，不是已上线服务。仓库有独立的 Python FRP 传输/授权参考实现，未与线上 Rust 项目注册表同步。手工 `frp_admin.py add` / `launch` 已禁用。拿到合法配置包后的 GPU/Docker 操作见 [供应商指南](HOSTING.md)，独立 FRPS 初始化见 [运营指南](FRP-HOSTING.md)。本地真实 FRP + 模拟模型传输已测试；真实 GPU 模型加载及生产 FRP 部署未验证。

## PRL 释放规则

My launches 区分 Locked、Free/withdrawable 与 Returned。当前合约只支持整笔退款或结算返还，没有部分解锁账本，因此不能将模拟 usage 变成真实可提取金额。

已确定、尚未实现的规则为：累计解锁 PRL = 初始 stake × 发布者成功交付并结算的 credits / 本次固定发行承诺总量。公共池代交付、失败及未结算请求不计入；重复结算不得重复释放。需要后续合约、结算服务和事件索引共同实现。

网站上的 10 页 PowerPoint 是产品概念白皮书，其中公共池补位、处罚和按履约比例释放 PRL 仍是设计目标。测试版购买、推理及 burn 的当前实现以 [使用 credits](CREDIT-SPENDING.md) 为准。
