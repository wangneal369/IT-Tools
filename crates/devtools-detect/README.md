# 智能识别内核

本 crate 复用 M1 的 `RawPayload`、`ToolId`、`MatchedDataChain`、`ParsedHint` 与 `ToolLaunchInput`；不拥有平台剪贴板，不依赖 GPUI、tokio 或 devtools-tools。

```rust
use std::sync::Arc;
use devtools_detect::{DetectionEngine, DetectionMode};
use devtools_detect::devtools_core::RawPayload;

let payload = RawPayload::Text(Arc::from(r#"[{"name":"example"}]"#));
let result = DetectionEngine::default().detect(&payload, DetectionMode::Strict);
for recommendation in result.banner() {
    let input = recommendation.launch_input(&payload);
    // 将 input 注入空标签或同指纹标签，其他情况由 UI 新建实例。
    assert!(!input.chain.nodes.is_empty());
}
```

- 平台先检查密码管理器隐藏标记，再读取一次快照；`detect_preferred` 为已经读取的多格式快照提供 Files > Image > Text 选择。
- `matches` 保留整条匹配路径，`recommendations` 应用六条压制并按置信度稳定排序、按 ToolId 去重。普通文件与特殊文件混合时仍保留 Hash 建议。
- `banner()` / `palette()` 仅显示 Medium+ 且未触发隐私抑制的前三 / 前五项。显式接力使用 `recommendations`，排除来源工具并在叶子之后追加一级父类型。
- 256 KiB 以内全量探测；其上至 2 MiB 仅验证前 64 KiB 魔数；超过 2 MiB 跳过且无预览。隐私检查扫描允许载荷全文以防密钥藏在前缀之外。
- 图片最多 20 MiB，探测只消费 64 KiB 头部；文件不递归、最多 64 项，每项最多读 64 KiB。超过 100 MiB 的文件不读正文，仅推荐流式 Hash。
- 硬预算 300 ms 在探测器与文件之间协作检查，可用 `with_budget` 收紧；解析器输入有长度与深度上限，YAML 限事件数、深度和文档数且拒绝别名/include。30 ms 是软目标，不是实时调度保证。同步文件系统调用无法强行抢占；调用方必须放在后台执行，平台应排除网络路径与设备等可能阻塞的来源。
- 证书只做 PEM/固定深度 DER 结构识别，不验签、不解密 PFX、不展开图像、不解压 GZip。DER 必须具有 X.509 的 TBS/签名结构，裸 PKCS#1/PKCS#8/SEC1 私钥与 CSR 不作为证书推荐。
- `ClipboardDeduplicator` 仅保存随机进程摘要、序号与可选首文件提示，支持内容去重和自我复制抑制。载荷、检测结果、推荐与去重器均不实现磁盘序列化。
