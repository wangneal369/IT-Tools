# 工具模糊检索

`CATALOG` 按 `ToolId::ALL` 同序定义全部 28 个核心 `ToolDescriptor`，包含中文显示名、英文名称、技术别名、常见缩写、拼音和首字母。不依赖 GPUI、tokio、检测或工具 crate。

```rust
use devtools_search::{SearchIndex, DetectionBoost};
use devtools_search::devtools_core::{Confidence, ToolId};

let mut index = SearchIndex::new();
assert_eq!(index.search("gsh")[0].descriptor.id, ToolId::JsonFormatter);
let recommendations = [DetectionBoost {
    tool_id: ToolId::JsonFormatter,
    confidence: Confidence::Exact,
}];
let results = index.search_with("", &recommendations, &[ToolId::Base64Codec]);
assert_eq!(results[0].descriptor.id, ToolId::JsonFormatter);
```

评分使用 `nucleo-matcher`，每个描述符取最佳字段分数，完全匹配和前缀匹配另加权。输入按 Unicode 字符限制为 256 字符，忽略大小写并去掉首尾空白。排序固定为：匹配查询的检测推荐（Medium+、去重、最多 5 项）→ 匹配的有序收藏 → 模糊分数降序 → 静态目录顺序。无查询、无提升时返回确定性的完整目录顺序。

调用者从 `devtools-detect::DetectionResult::palette()` 映射到 `DetectionBoost`，即可沿用隐私抑制和展示阈值；本 crate 不读取剪贴板载荷。匹配器复用内部缓冲区，每任务独立创建实例或由调用方串行访问。

规格选定的 `nucleo-matcher 0.3.1` 为 MPL-2.0，根 `deny.toml` 仅针对此版本声明许可证例外；使用未修改上游依赖，不复制其源文件到本项目。
