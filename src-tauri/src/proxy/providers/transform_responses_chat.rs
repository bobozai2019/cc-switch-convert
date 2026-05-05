use crate::proxy::ProxyError;
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Map, Value};

fn as_array_mut(value: &mut Value) -> Option<&mut Vec<Value>> {
    value.as_array_mut()
}

fn responses_tools_to_chat_tools(tools: &Value) -> Value {
    let Some(arr) = tools.as_array() else {
        return tools.clone();
    };
    Value::Array(
        arr.iter()
            .filter_map(|tool| {
                if tool.get("type").and_then(Value::as_str) != Some("function") {
                    return None;
                }
                let name = tool.get("name").and_then(Value::as_str)
                    .or_else(|| tool.pointer("/function/name").and_then(Value::as_str));
                if name.is_none() || name == Some("") {
                    return None;
                }

                Some(if tool.get("function").is_none() {
                    json!({
                        "type": "function",
                        "function": {
                            "name": name.unwrap_or(""),
                            "description": tool.get("description").cloned().unwrap_or(json!("")),
                            "parameters": tool.get("parameters").cloned().unwrap_or(json!({}))
                        }
                    })
                } else {
                    tool.clone()
                })
            })
            .collect(),
    )
}

fn chat_tools_to_responses_tools(tools: &Value) -> Value {
    let Some(arr) = tools.as_array() else {
        return tools.clone();
    };
    Value::Array(
        arr.iter()
            .map(|tool| {
                if tool.get("type").and_then(Value::as_str) == Some("function") {
                    if let Some(func) = tool.get("function") {
                        json!({
                            "type": "function",
                            "name": func.get("name").cloned().unwrap_or(json!("function")),
                            "description": func.get("description").cloned().unwrap_or(json!("")),
                            "parameters": func.get("parameters").cloned().unwrap_or(json!({}))
                        })
                    } else {
                        tool.clone()
                    }
                } else {
                    tool.clone()
                }
            })
            .collect(),
    )
}

fn responses_tool_choice_to_chat(choice: &Value) -> Option<Value> {
    if choice.as_str().is_some() {
        return Some(choice.clone());
    }
    if choice.get("type").and_then(Value::as_str) == Some("function")
        && choice.get("function").is_none()
    {
        let name = choice.get("name").and_then(Value::as_str)?;
        if name.is_empty() {
            return None;
        }
        return Some(json!({
            "type": "function",
            "function": {
                "name": name
            }
        }));
    }
    if choice.get("type").and_then(Value::as_str) == Some("function") {
        return Some(choice.clone());
    }
    None
}

fn responses_role_to_chat_role(role: &str) -> &str {
    match role {
        "developer" => "system",
        other => other,
    }
}

fn responses_content_to_chat_content(content: &Value) -> Value {
    let Some(parts) = content.as_array() else {
        return content.clone();
    };

    let text = parts
        .iter()
        .filter_map(|part| {
            part.get("text")
                .or_else(|| part.get("content"))
                .and_then(Value::as_str)
        })
        .collect::<Vec<_>>()
        .join("");

    Value::String(text)
}

fn responses_message_to_chat_message(item: &Value) -> Option<Value> {
    let role = item.get("role").and_then(Value::as_str)?;
    let role = responses_role_to_chat_role(role);
    let content = item
        .get("content")
        .map(responses_content_to_chat_content)
        .unwrap_or_else(|| json!(""));
    Some(json!({
        "role": role,
        "content": content
    }))
}

fn chat_tool_choice_to_responses(choice: &Value) -> Value {
    if choice.get("type").and_then(Value::as_str) == Some("function") {
        if let Some(name) = choice.pointer("/function/name").cloned() {
            return json!({
                "type": "function",
                "name": name
            });
        }
    }
    choice.clone()
}

pub fn responses_to_chat_request(mut body: Value) -> Result<Value, ProxyError> {
    let mut out = Map::new();
    if let Some(model) = body.get("model").cloned() {
        out.insert("model".to_string(), model);
    }
    if let Some(stream) = body.get("stream").cloned() {
        out.insert("stream".to_string(), stream);
    }
    if let Some(temp) = body.get("temperature").cloned() {
        out.insert("temperature".to_string(), temp);
    }
    if let Some(top_p) = body.get("top_p").cloned() {
        out.insert("top_p".to_string(), top_p);
    }
    if let Some(max) = body.get("max_output_tokens").cloned() {
        out.insert("max_tokens".to_string(), max);
    }
    if let Some(re) = body.pointer("/reasoning/effort").cloned() {
        out.insert("reasoning_effort".to_string(), re);
    }
    if let Some(tc) = body.get("tool_choice").cloned() {
        if let Some(mapped) = responses_tool_choice_to_chat(&tc) {
            out.insert("tool_choice".to_string(), mapped);
        }
    }
    if let Some(tools) = body.get("tools").cloned() {
        out.insert("tools".to_string(), responses_tools_to_chat_tools(&tools));
    }

    let mut messages = Vec::new();
    if let Some(instr) = body.get("instructions").and_then(|v| v.as_str()) {
        messages.push(json!({"role":"system","content":instr}));
    }
    if let Some(input) = body.get_mut("input") {
        if let Some(s) = input.as_str() {
            messages.push(json!({"role":"user","content":s}));
        } else if input.is_object() {
            messages.push(input.clone());
        } else if let Some(items) = as_array_mut(input) {
            for item in items.iter() {
                if item.get("type").and_then(Value::as_str) == Some("function_call_output") {
                    messages.push(json!({
                        "role":"tool",
                        "tool_call_id": item.get("call_id").cloned().unwrap_or(json!("")),
                        "content": item.get("output").cloned().unwrap_or(json!(""))
                    }));
                } else if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                    if name.is_empty() {
                        continue;
                    }
                    messages.push(json!({
                        "role":"assistant",
                        "tool_calls":[{"id": item.get("call_id").cloned().unwrap_or(json!("call_1")),
                        "type":"function","function":{
                            "name": name,
                            "arguments": item.get("arguments").cloned().unwrap_or(json!("{}"))
                        }}]
                    }));
                } else if let Some(message) = responses_message_to_chat_message(item) {
                    messages.push(message);
                }
            }
        }
    }
    out.insert("messages".to_string(), Value::Array(messages));
    Ok(Value::Object(out))
}

pub fn chat_to_responses_request(mut body: Value) -> Result<Value, ProxyError> {
    let mut out = Map::new();
    if let Some(model) = body.get("model").cloned() {
        out.insert("model".to_string(), model);
    }
    if let Some(stream) = body.get("stream").cloned() {
        out.insert("stream".to_string(), stream);
    }
    if let Some(temp) = body.get("temperature").cloned() {
        out.insert("temperature".to_string(), temp);
    }
    if let Some(top_p) = body.get("top_p").cloned() {
        out.insert("top_p".to_string(), top_p);
    }
    if let Some(max) = body
        .get("max_completion_tokens")
        .cloned()
        .or_else(|| body.get("max_tokens").cloned())
    {
        out.insert("max_output_tokens".to_string(), max);
    }
    if let Some(re) = body.get("reasoning_effort").cloned() {
        out.insert("reasoning".to_string(), json!({ "effort": re }));
    }
    if let Some(tc) = body.get("tool_choice").cloned() {
        out.insert("tool_choice".to_string(), chat_tool_choice_to_responses(&tc));
    }
    if let Some(tools) = body.get("tools").cloned() {
        out.insert("tools".to_string(), chat_tools_to_responses_tools(&tools));
    }

    let mut input = Vec::new();
    let mut instructions: Vec<String> = Vec::new();
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        for msg in messages {
            if msg.get("role").and_then(Value::as_str) == Some("system") {
                if let Some(c) = msg.get("content").and_then(Value::as_str) {
                    instructions.push(c.to_string());
                }
                continue;
            }
            input.push(msg.clone());
        }
    }
    if !instructions.is_empty() {
        out.insert("instructions".to_string(), Value::String(instructions.join("\n")));
    }
    out.insert("input".to_string(), Value::Array(input));
    Ok(Value::Object(out))
}

pub fn chat_to_responses_response(body: Value) -> Result<Value, ProxyError> {
    let id = body.get("id").cloned().unwrap_or(json!("resp_chat"));
    let model = body.get("model").cloned().unwrap_or(json!("unknown"));
    let choice = body.pointer("/choices/0").cloned().unwrap_or_else(|| json!({}));
    let message = choice.get("message").cloned().unwrap_or_else(|| json!({}));
    let finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("stop");
    let mut output = Vec::new();
    if let Some(content) = message.get("content").cloned() {
        output.push(json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":content}]}));
    }
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            output.push(json!({
                "type":"function_call",
                "call_id": call.get("id").cloned().unwrap_or(json!("call_1")),
                "name": call.pointer("/function/name").cloned().unwrap_or(json!("function")),
                "arguments": call.pointer("/function/arguments").cloned().unwrap_or(json!("{}"))
            }));
        }
    }
    let status = if finish_reason == "length" {
        "incomplete"
    } else {
        "completed"
    };
    let mut resp = json!({"id":id,"object":"response","model":model,"status":status,"output":output});
    if finish_reason == "length" {
        resp["incomplete_details"] = json!({"reason":"max_output_tokens"});
    }
    if let Some(u) = body.get("usage") {
        resp["usage"] = json!({
            "input_tokens": u.get("prompt_tokens").cloned().unwrap_or(json!(0)),
            "output_tokens": u.get("completion_tokens").cloned().unwrap_or(json!(0)),
            "input_tokens_details": { "cached_tokens": u.pointer("/prompt_tokens_details/cached_tokens").cloned().unwrap_or(json!(0)) }
        });
    }
    Ok(resp)
}

pub fn responses_to_chat_response(body: Value) -> Result<Value, ProxyError> {
    let id = body.get("id").cloned().unwrap_or(json!("chatcmpl_resp"));
    let model = body.get("model").cloned().unwrap_or(json!("unknown"));
    let output = body.get("output").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut content = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    for item in output {
        if item.get("type").and_then(Value::as_str) == Some("message") {
            if let Some(parts) = item.get("content").and_then(Value::as_array) {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        content.push_str(text);
                    }
                }
            }
        } else if item.get("type").and_then(Value::as_str) == Some("function_call") {
            tool_calls.push(json!({
                "id": item.get("call_id").cloned().unwrap_or(json!("call_1")),
                "type":"function",
                "function":{
                    "name": item.get("name").cloned().unwrap_or(json!("function")),
                    "arguments": item.get("arguments").cloned().unwrap_or(json!("{}"))
                }
            }));
        }
    }
    let status = body.get("status").and_then(Value::as_str).unwrap_or("completed");
    let finish_reason = if status == "incomplete"
        && body.pointer("/incomplete_details/reason").and_then(Value::as_str) == Some("max_output_tokens")
    {
        "length"
    } else if !tool_calls.is_empty() {
        "tool_calls"
    } else {
        "stop"
    };
    let mut resp = json!({
        "id": id, "object":"chat.completion", "model": model,
        "choices":[{"index":0,"message":{"role":"assistant","content":content},"finish_reason":finish_reason}]
    });
    if !tool_calls.is_empty() {
        resp["choices"][0]["message"]["tool_calls"] = Value::Array(tool_calls);
    }
    if let Some(u) = body.get("usage") {
        resp["usage"] = json!({
            "prompt_tokens": u.get("input_tokens").cloned().unwrap_or(json!(0)),
            "completion_tokens": u.get("output_tokens").cloned().unwrap_or(json!(0)),
            "total_tokens": u.get("input_tokens").and_then(Value::as_i64).unwrap_or(0) + u.get("output_tokens").and_then(Value::as_i64).unwrap_or(0)
        });
    }
    Ok(resp)
}

pub fn chat_sse_to_responses_sse<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut rem = Vec::new();
        let mut response_id = "resp_chat_stream".to_string();
        let mut model = "unknown".to_string();
        let message_id = "msg_chat_0";
        let mut created_sent = false;
        let mut text_item_sent = false;
        let mut text_done_sent = false;
        let mut completed_sent = false;
        let mut output_text = String::new();
        tokio::pin!(stream);
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut rem, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        let mut data_line: Option<&str> = None;
                        for line in block.lines() {
                            if let Some(d) = strip_sse_field(line, "data") {
                                data_line = Some(d);
                                break;
                            }
                        }
                        let Some(data) = data_line else { continue; };
                        if data.trim() == "[DONE]" {
                            if !completed_sent {
                                let completed = json!({
                                    "type":"response.completed",
                                    "response":{
                                        "id":response_id,
                                        "object":"response",
                                        "model":model,
                                        "status":"completed",
                                        "output":[{"id":message_id,"type":"message","role":"assistant","content":[{"type":"output_text","text":output_text}]}],
                                        "usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}
                                    }
                                });
                                yield Ok(Bytes::from(format!("event: response.completed\ndata: {}\n\n", completed)));
                            }
                            yield Ok(Bytes::from("data: [DONE]\n\n".to_string()));
                            continue;
                        }
                        let v: Value = match serde_json::from_str(data) { Ok(v)=>v, Err(_)=>continue };
                        if let Some(id) = v.get("id").and_then(Value::as_str) {
                            response_id = id.to_string();
                        }
                        if let Some(m) = v.get("model").and_then(Value::as_str) {
                            model = m.to_string();
                        }
                        if !created_sent {
                            let created = json!({
                                "type":"response.created",
                                "response":{
                                    "id":response_id,
                                    "object":"response",
                                    "model":model,
                                    "status":"in_progress",
                                    "output":[]
                                }
                            });
                            yield Ok(Bytes::from(format!("event: response.created\ndata: {}\n\n", created)));
                            created_sent = true;
                        }
                        let delta = v.pointer("/choices/0/delta").cloned().unwrap_or_else(|| json!({}));
                        if let Some(content) = delta.get("content").and_then(Value::as_str) {
                            if !text_item_sent {
                                let added = json!({
                                    "type":"response.output_item.added",
                                    "output_index":0,
                                    "item":{"id":message_id,"type":"message","status":"in_progress","role":"assistant","content":[]}
                                });
                                yield Ok(Bytes::from(format!("event: response.output_item.added\ndata: {}\n\n", added)));
                                let part = json!({
                                    "type":"response.content_part.added",
                                    "item_id":message_id,
                                    "output_index":0,
                                    "content_index":0,
                                    "part":{"type":"output_text","text":""}
                                });
                                yield Ok(Bytes::from(format!("event: response.content_part.added\ndata: {}\n\n", part)));
                                text_item_sent = true;
                            }
                            output_text.push_str(content);
                            let out = json!({"type":"response.output_text.delta","item_id":message_id,"output_index":0,"content_index":0,"delta":content});
                            yield Ok(Bytes::from(format!("event: response.output_text.delta\ndata: {}\n\n", out)));
                        }
                        if let Some(tcalls) = delta.get("tool_calls").and_then(Value::as_array) {
                            for call in tcalls {
                                let call_id = call.get("id").and_then(Value::as_str).unwrap_or("");
                                let fn_name = call.pointer("/function/name").and_then(Value::as_str).unwrap_or("");
                                let fn_args = call.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("");
                                if call_id.is_empty() && fn_name.is_empty() && fn_args.is_empty() {
                                    continue;
                                }
                                let stable_call_id = if call_id.is_empty() { "call_1" } else { call_id };
                                let stable_name = if fn_name.is_empty() { "function" } else { fn_name };
                                let added = json!({"type":"response.output_item.added","item":{"type":"function_call","call_id":stable_call_id,"name":stable_name,"arguments":""}});
                                yield Ok(Bytes::from(format!("event: response.output_item.added\ndata: {}\n\n", added)));
                                if !fn_args.is_empty() {
                                    let a = json!({"type":"response.function_call_arguments.delta","delta":fn_args});
                                    yield Ok(Bytes::from(format!("event: response.function_call_arguments.delta\ndata: {}\n\n", a)));
                                }
                            }
                        }
                        if let Some(fr) = v.pointer("/choices/0/finish_reason").and_then(Value::as_str) {
                            if text_item_sent && !text_done_sent {
                                let part_done = json!({"type":"response.content_part.done","item_id":message_id,"output_index":0,"content_index":0});
                                yield Ok(Bytes::from(format!("event: response.content_part.done\ndata: {}\n\n", part_done)));
                                let item_done = json!({
                                    "type":"response.output_item.done",
                                    "output_index":0,
                                    "item":{"id":message_id,"type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":output_text}]}
                                });
                                yield Ok(Bytes::from(format!("event: response.output_item.done\ndata: {}\n\n", item_done)));
                                text_done_sent = true;
                            }
                            let usage = v.get("usage").cloned().unwrap_or_else(|| json!({}));
                            let input_tokens = usage.get("prompt_tokens").and_then(Value::as_i64).unwrap_or(0);
                            let output_tokens = usage.get("completion_tokens").and_then(Value::as_i64).unwrap_or(0);
                            let status = if fr == "length" { "incomplete" } else { "completed" };
                            let completed = json!({
                                "type":"response.completed",
                                "response":{
                                    "id":response_id,
                                    "object":"response",
                                    "model":model,
                                    "status":status,
                                    "output":[{"id":message_id,"type":"message","role":"assistant","content":[{"type":"output_text","text":output_text}]}],
                                    "usage":{"input_tokens":input_tokens,"output_tokens":output_tokens,"total_tokens":input_tokens + output_tokens}
                                }
                            });
                            yield Ok(Bytes::from(format!("event: response.completed\ndata: {}\n\n", completed)));
                            completed_sent = true;
                        }
                    }
                }
                Err(e) => {
                    yield Err(std::io::Error::other(e.to_string()));
                    break;
                }
            }
        }
    }
}

pub fn responses_sse_to_chat_sse<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut rem = Vec::new();
        tokio::pin!(stream);
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut rem, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        let mut event_name = "";
                        let mut data_line: Option<&str> = None;
                        for line in block.lines() {
                            if let Some(e) = strip_sse_field(line, "event") { event_name = e.trim(); }
                            if let Some(d) = strip_sse_field(line, "data") { data_line = Some(d); }
                        }
                        let Some(data) = data_line else { continue; };
                        if data.trim() == "[DONE]" {
                            yield Ok(Bytes::from("data: [DONE]\n\n".to_string()));
                            continue;
                        }
                        let v: Value = match serde_json::from_str(data) { Ok(v)=>v, Err(_)=>continue };
                        match event_name {
                            "response.output_text.delta" => {
                                let delta = v.get("delta").cloned().unwrap_or(json!(""));
                                let out = json!({"id":"chatcmpl_bridge","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":delta}}]});
                                yield Ok(Bytes::from(format!("data: {}\n\n", out)));
                            }
                            "response.output_item.added" => {
                                let tc = json!({"index":0,"id":v.pointer("/item/call_id").cloned().unwrap_or(json!("call_1")),"type":"function","function":{"name":v.pointer("/item/name").cloned().unwrap_or(json!("function")),"arguments":""}});
                                let out = json!({"id":"chatcmpl_bridge","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[tc]}}]});
                                yield Ok(Bytes::from(format!("data: {}\n\n", out)));
                            }
                            "response.function_call_arguments.delta" => {
                                let tc = json!({"index":0,"function":{"arguments":v.get("delta").cloned().unwrap_or(json!(""))}});
                                let out = json!({"id":"chatcmpl_bridge","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[tc]}}]});
                                yield Ok(Bytes::from(format!("data: {}\n\n", out)));
                            }
                            "response.completed" => {
                                let usage = v.pointer("/response/usage").cloned().unwrap_or_else(|| json!({}));
                                let out = json!({"id":"chatcmpl_bridge","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":usage.get("input_tokens").cloned().unwrap_or(json!(0)),"completion_tokens":usage.get("output_tokens").cloned().unwrap_or(json!(0))}});
                                yield Ok(Bytes::from(format!("data: {}\n\n", out)));
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    yield Err(std::io::Error::other(e.to_string()));
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use futures::StreamExt;

    #[test]
    fn responses_chat_request_basic_mapping() {
        let r = responses_to_chat_request(json!({"model":"m1","instructions":"sys","input":[{"role":"user","content":"hi"}],"max_output_tokens":10})).unwrap();
        assert_eq!(r["model"], "m1");
        assert_eq!(r["messages"][0]["role"], "system");
    }

    #[test]
    fn chat_responses_response_basic_mapping() {
        let r = chat_to_responses_response(json!({"id":"c1","model":"m1","choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]})).unwrap();
        assert_eq!(r["id"], "c1");
        assert_eq!(r["status"], "completed");
    }

    #[test]
    fn responses_to_chat_request_supports_string_input() {
        let r = responses_to_chat_request(json!({"model":"m","input":"hello"})).unwrap();
        assert_eq!(r["messages"][0]["role"], "user");
    }

    #[test]
    fn responses_to_chat_request_maps_function_tools_shape() {
        let r = responses_to_chat_request(json!({
            "model":"m",
            "input":"hi",
            "tools":[{"type":"function","name":"get_weather","description":"d","parameters":{"type":"object"}}],
            "tool_choice":{"type":"function","name":"get_weather"}
        }))
        .unwrap();
        assert_eq!(r["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(r["tool_choice"]["function"]["name"], "get_weather");
    }

    #[test]
    fn chat_to_responses_request_maps_function_tools_shape() {
        let r = chat_to_responses_request(json!({
            "model":"m",
            "messages":[{"role":"user","content":"hi"}],
            "tools":[{"type":"function","function":{"name":"get_weather","description":"d","parameters":{"type":"object"}}}],
            "tool_choice":{"type":"function","function":{"name":"get_weather"}}
        }))
        .unwrap();
        assert_eq!(r["tools"][0]["name"], "get_weather");
        assert_eq!(r["tool_choice"]["name"], "get_weather");
    }

    #[test]
    fn responses_to_chat_request_drops_non_function_tools() {
        let r = responses_to_chat_request(json!({
            "model":"m",
            "input":"hi",
            "tools":[
                {"type":"function","name":"get_weather","parameters":{"type":"object"}},
                {"type":"web_search"}
            ],
            "tool_choice":{"type":"web_search"}
        }))
        .unwrap();
        assert_eq!(r["tools"].as_array().unwrap().len(), 1);
        assert_eq!(r["tools"][0]["type"], "function");
        assert!(r.get("tool_choice").is_none());
    }

    #[test]
    fn responses_to_chat_request_drops_nameless_function_tools() {
        let r = responses_to_chat_request(json!({
            "model":"m",
            "input":"hi",
            "tools":[
                {"type":"function","description":"bad tool without name"},
                {"type":"function","name":"ok_tool","parameters":{"type":"object"}}
            ]
        }))
        .unwrap();
        let tools = r["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].pointer("/function/name").and_then(Value::as_str), Some("ok_tool"));
    }

    #[test]
    fn responses_to_chat_request_maps_developer_role_to_system() {
        let r = responses_to_chat_request(json!({
            "model":"m",
            "input":[
                {"type":"message","role":"developer","content":[{"type":"input_text","text":"be brief"}]},
                {"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}
            ]
        }))
        .unwrap();
        assert_eq!(r["messages"][0]["role"], "system");
        assert_eq!(r["messages"][0]["content"], "be brief");
        assert_eq!(r["messages"][1]["role"], "user");
    }

    #[tokio::test]
    async fn chat_sse_to_responses_sse_maps_text_delta() {
        let input = "data: {\"id\":\"c1\",\"model\":\"m1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n\
data: {\"id\":\"c1\",\"model\":\"m1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1}}\n\n\
data: [DONE]\n\n";
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(input.as_bytes().to_vec()))]);
        let out = chat_sse_to_responses_sse(upstream);
        let chunks: Vec<_> = out.collect().await;
        let merged = chunks.into_iter().map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string()).collect::<String>();
        assert!(merged.contains("event: response.created"));
        assert!(merged.contains("event: response.output_item.added"));
        assert!(merged.contains("event: response.content_part.added"));
        assert!(merged.contains("event: response.output_text.delta"));
        assert!(merged.contains("\"delta\":\"hi\""));
        assert!(merged.contains("event: response.completed"));
        assert!(merged.contains("\"id\":\"c1\""));
    }

    #[tokio::test]
    async fn responses_sse_to_chat_sse_maps_text_delta() {
        let input = "event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":3,\"output_tokens\":1}}}\n\n\
data: [DONE]\n\n";
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(input.as_bytes().to_vec()))]);
        let out = responses_sse_to_chat_sse(upstream);
        let chunks: Vec<_> = out.collect().await;
        let merged = chunks.into_iter().map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string()).collect::<String>();
        assert!(merged.contains("\"content\":\"ok\""));
        assert!(merged.contains("\"finish_reason\":\"stop\""));
        assert!(merged.contains("data: [DONE]"));
    }
}
