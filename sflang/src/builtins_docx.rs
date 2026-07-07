//! builtins_docx.rs — Word (docx) 文档处理内置函数
//!
//! docx 本质是 ZIP 包，内部 word/document.xml 存放正文。
//! 通过 zip crate 解压/压缩，用字符串操作处理 XML 中的文本。
//!
//! 函数（对标 Charlang）：
//!   docxToStrs(path)                    — 提取段落文本为字符串数组
//!   docxReplace(bytes, [旧值, 新值...])  — 批量替换文本（模板填充），返回新 bytes
//!   docxGetPlaceholders(bytes)           — 提取文档中的占位符

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use crate::builtins_helpers as bh;
use crate::value::Value;

/// register 注册所有 docx 内置函数。
pub fn register(vm: &mut crate::vm::VM) {
    vm.register_builtin("docxToStrs", bi_docx_to_strs);
    vm.register_builtin("docxReplace", bi_docx_replace);
    vm.register_builtin("docxGetPlaceholders", bi_docx_get_placeholders);
}

// ---- 辅助函数 ----

/// docx_extract_document_xml 从 docx 字节中提取 word/document.xml 的内容。
///
/// 失败返回错误值。
fn extract_document_xml(docx_bytes: &[u8]) -> Result<String, Value> {
    let cursor = std::io::Cursor::new(docx_bytes.to_vec());
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| {
        crate::value::error_value(format!(
            "docx 解压失败: {} (可能原因：不是有效的 docx 文件)", e,
        ))
    })?;

    // 查找 word/document.xml
    let mut doc_xml = String::new();
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|e| {
            crate::value::error_value(format!("docx 读取内部文件失败: {}", e))
        })?;
        if file.name() == "word/document.xml" {
            let mut reader = file;
            reader.read_to_string(&mut doc_xml).map_err(|e| {
                crate::value::error_value(format!(
                    "docx 读取 document.xml 失败: {} (可能原因：编码问题)", e,
                ))
            })?;
            break;
        }
    }

    if doc_xml.is_empty() {
        return Err(crate::value::error_value(
            "docx 中未找到 word/document.xml (可能原因：不是标准的 .docx 文件)",
        ));
    }

    Ok(doc_xml)
}

/// extract_text_from_xml 从 document.xml 中提取纯文本。
///
/// docx 的文本在 <w:t>...</w:t> 标签中。
/// <w:p> 表示段落，每个段落拼接为一个字符串。
/// 返回段落字符串数组。
fn extract_text_from_xml(xml: &str) -> Vec<String> {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current_para = String::new();

    // 按字符遍历，跟踪标签状态
    let bytes = xml.as_bytes();
    let mut i = 0;
    let mut in_tag = false;
    let mut tag_start = 0;

    while i < bytes.len() {
        if !in_tag {
            if bytes[i] == b'<' {
                in_tag = true;
                tag_start = i;
            }
            i += 1;
        } else {
            // 在标签内，找 >
            if bytes[i] == b'>' {
                let tag = &xml[tag_start..=i];
                in_tag = false;

                // 段落结束标签
                if tag.starts_with("</w:p") {
                    paragraphs.push(std::mem::take(&mut current_para));
                }
                // 文本开始标签 <w:t> 或 <w:t ...>
                else if tag.starts_with("<w:t") && !tag.starts_with("</w:t") {
                    // 提取 > 之后的文本直到 </w:t>
                    // 注意 xml:space="preserve" 等属性
                    let text_start = i + 1;
                    // 找到 </w:t>
                    if let Some(end) = xml[text_start..].find("</w:t>") {
                        let text = &xml[text_start..text_start + end];
                        // 解码基本 XML 实体
                        current_para.push_str(&decode_xml_entities(text));
                        i = text_start + end + "</w:t>".len();
                        continue;
                    }
                }
            }
            i += 1;
        }
    }

    // 处理最后一个未闭合的段落
    if !current_para.is_empty() {
        paragraphs.push(current_para);
    }

    paragraphs
}

/// decode_xml_entities 解码基本 XML 实体。
fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// encode_xml_entities 编码文本为 XML 安全格式。
fn encode_xml_entities(s: &str) -> String {
    s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
}

/// docx_replace_in_xml 在 document.xml 中批量替换文本。
///
/// 每对 (old, new) 在 <w:t> 标签内的文本中替换。
/// 简单实现：直接在整个 XML 字符串上做替换（docx 的文本通常完整在单个 <w:t> 内）。
fn replace_in_xml(xml: &str, pairs: &[(String, String)]) -> String {
    let mut result = xml.to_string();
    for (old, new) in pairs {
        // 编码后的旧值和新值（因为 XML 中 & 是 &amp; 等）
        let old_encoded = encode_xml_entities(old);
        let new_encoded = encode_xml_entities(new);
        result = result.replace(&old_encoded, &new_encoded);
        // 也尝试未编码的替换（以防标签内文本未被编码）
        if old_encoded != *old {
            result = result.replace(old, &new_encoded);
        }
    }
    result
}

/// rebuild_docx 在原 docx 字节基础上，替换 document.xml 内容，重新打包。
fn rebuild_docx(docx_bytes: &[u8], new_doc_xml: &str) -> Result<Vec<u8>, Value> {
    let cursor = std::io::Cursor::new(docx_bytes.to_vec());
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| {
        crate::value::error_value(format!("docx 解压失败: {}", e))
    })?;

    let mut output = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut output);
        let options = zip::write::SimpleFileOptions::default();

        // 复制所有文件，替换 document.xml
        for i in 0..archive.len() {
            let mut file = archive.by_index_raw(i).map_err(|e| {
                crate::value::error_value(format!("docx 读取文件失败: {}", e))
            })?;
            let name = file.name().to_string();

            writer.start_file(&name, options).map_err(|e| {
                crate::value::error_value(format!("docx 写入文件失败: {}", e))
            })?;

            if name == "word/document.xml" {
                writer.write_all(new_doc_xml.as_bytes()).map_err(|e| {
                    crate::value::error_value(format!("docx 写入 document.xml 失败: {}", e))
                })?;
            } else {
                std::io::copy(&mut file, &mut writer).map_err(|e| {
                    crate::value::error_value(format!("docx 复制文件失败: {}", e))
                })?;
            }
        }
        writer.finish().map_err(|e| {
            crate::value::error_value(format!("docx 压缩失败: {}", e))
        })?;
    }

    Ok(output.into_inner())
}

// ---- 内置函数 ----

/// bi_docx_to_strs 提取 docx 段落文本为字符串数组。
///
/// 用法：docxToStrs(path) → ["段落1", "段落2", ...]
fn bi_docx_to_strs(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    // 支持 path 或 bytes
    let docx_bytes: Vec<u8> = match &args[0] {
        Value::Str(path) => std::fs::read(path.as_ref()).map_err(|e| {
            crate::value::error_value(format!(
                "docxToStrs() 读取文件 '{}' 失败: {} (可能原因：文件不存在)", path, e,
            ))
        })?,
        Value::Bytes(b) => b.as_ref().to_vec(),
        other => return Err(crate::value::error_value(format!(
            "docxToStrs() 参数应为 path(string) 或 bytes，得到 {}", other.type_name(),
        ))),
    };

    let xml = extract_document_xml(&docx_bytes)?;
    let paragraphs = extract_text_from_xml(&xml);

    let result: Vec<Value> = paragraphs.into_iter().map(Value::str_from).collect();
    Ok(Value::Array(Arc::new(Mutex::new(result))))
}

/// bi_docx_replace 批量替换 docx 中的文本（模板填充），返回新的 bytes。
///
/// 用法：docxReplace(bytes, [旧值1, 新值1, 旧值2, 新值2, ...]) → bytes
fn bi_docx_replace(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "docxReplace")?;
    bh::require_arg(args, 1, "docxReplace")?;

    let docx_bytes: Vec<u8> = match &args[0] {
        Value::Bytes(b) => b.as_ref().to_vec(),
        other => return Err(crate::value::error_value(format!(
            "docxReplace() 第 1 个参数应为 bytes，得到 {}", other.type_name(),
        ))),
    };

    // 解析替换对
    let pairs_vec = match &args[1] {
        Value::Array(a) => a.lock().unwrap().clone(),
        other => return Err(crate::value::error_value(format!(
            "docxReplace() 第 2 个参数应为数组 [旧值, 新值, ...]，得到 {}", other.type_name(),
        ))),
    };

    // 两两配对
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i + 1 < pairs_vec.len() {
        pairs.push((pairs_vec[i].to_str(), pairs_vec[i + 1].to_str()));
        i += 2;
    }

    let xml = extract_document_xml(&docx_bytes)?;
    let new_xml = replace_in_xml(&xml, &pairs);
    let new_bytes = rebuild_docx(&docx_bytes, &new_xml)?;

    Ok(Value::Bytes(Arc::new(new_bytes)))
}

/// bi_docx_get_placeholders 提取 docx 中的占位符。
///
/// 占位符格式：{name}、{{name}}、${name} 等（花括号包裹的文本）。
///
/// 用法：docxGetPlaceholders(bytes) → ["{name}", "{date}", ...]
fn bi_docx_get_placeholders(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "docxGetPlaceholders")?;

    let docx_bytes: Vec<u8> = match &args[0] {
        Value::Bytes(b) => b.as_ref().to_vec(),
        Value::Str(path) => std::fs::read(path.as_ref()).map_err(|e| {
            crate::value::error_value(format!(
                "docxGetPlaceholders() 读取文件 '{}' 失败: {}", path, e,
            ))
        })?,
        other => return Err(crate::value::error_value(format!(
            "docxGetPlaceholders() 参数应为 bytes 或 path(string)，得到 {}", other.type_name(),
        ))),
    };

    let xml = extract_document_xml(&docx_bytes)?;
    let paragraphs = extract_text_from_xml(&xml);
    let full_text = paragraphs.join("");

    // 提取 {xxx} 格式的占位符（排除 XML 标签里的花括号）
    let mut placeholders: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let chars: Vec<char> = full_text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            // 找匹配的 }
            if let Some(end) = chars[i + 1..].iter().position(|&c| c == '}') {
                let placeholder: String = chars[i..=i + 1 + end].iter().collect();
                // 过滤掉太短或含 < > 的（可能是 XML 残留）
                let inner = &placeholder[1..placeholder.len() - 1];
                if !inner.is_empty() && !inner.contains('<') && !inner.contains('>') {
                    if seen.insert(placeholder.clone()) {
                        placeholders.push(placeholder);
                    }
                }
                i = i + 1 + end + 1;
                continue;
            }
        }
        i += 1;
    }

    let result: Vec<Value> = placeholders.into_iter().map(Value::str_from).collect();
    Ok(Value::Array(Arc::new(Mutex::new(result))))
}
