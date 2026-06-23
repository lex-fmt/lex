//! IR → JSON transform.
//!
//! Backs the `ir-json` transform: a JSON rendering of the intermediate
//! parse-tree (`ParseNode`).

use super::tokens::tokens_to_json;

/// Convert IR (ParseNode) to JSON-serializable format
pub(super) fn ir_to_json(node: &lex_core::lex::parsing::ir::ParseNode) -> serde_json::Value {
    use serde_json::json;

    json!({
        "type": format!("{:?}", node.node_type),
        "tokens": tokens_to_json(&node.tokens),
        "children": node.children.iter().map(ir_to_json).collect::<Vec<_>>(),
        "has_payload": node.payload.is_some(),
    })
}
