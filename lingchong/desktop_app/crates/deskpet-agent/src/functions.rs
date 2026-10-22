//! LLM Function Calling 工具定义。
//!
//! Agent 可调用的工具：
//!   - fetch_hot_detail：拉取热点详情（来自本地库，可溯源）
//!   - compose_announcement：基于画像生成拟人化口播文案

use serde_json::Value;

/// 工具的 JSON Schema（传给 LLM 的 tools 字段）。
pub fn tool_schemas() -> Vec<Value> {
    vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "fetch_hot_detail",
                "description": "根据热点 id 获取该热点的详细信息，包括标题、链接、摘要、相关相似热点。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "hot_id": { "type": "string", "description": "热点唯一 id" }
                    },
                    "required": ["hot_id"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "compose_announcement",
                "description": "根据热点详情与主人画像，生成一段拟人化的口播文案。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "hot_id": { "type": "string" },
                        "persona": { "type": "string", "description": "主人画像描述" }
                    },
                    "required": ["hot_id", "persona"]
                }
            }
        }),
    ]
}

/// 本地执行工具。返回 (tool_name, result_json)。
pub fn execute_tool(name: &str, args: &Value, conn: &rusqlite::Connection) -> Option<Value> {
    match name {
        "fetch_hot_detail" => {
            let id = args["hot_id"].as_str()?;
            let item = deskpet_storage::HotRepo::get(conn, id).ok().flatten()?;
            Some(serde_json::json!({
                "id": item.id,
                "platform": item.platform,
                "title": item.title,
                "url": item.url,
                "summary": item.summary,
                "tags": item.tags,
                "hot_value": item.hot_value,
            }))
        }
        "compose_announcement" => {
            // 该工具实际由 LLM 在对话中直接生成文案，这里返回模板提示
            let id = args["hot_id"].as_str().unwrap_or("");
            let persona = args["persona"].as_str().unwrap_or("");
            Some(serde_json::json!({
                "instruction": format!(
                    "请用宠物的口吻，根据主人画像「{}」，为热点 id={} 写一段 1-2 句口播，要说明是什么、为什么跟主人有关、附原文链接。",
                    persona, id
                )
            }))
        }
        _ => None,
    }
}
