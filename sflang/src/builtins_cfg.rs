//! builtins_cfg.rs — 持久化配置存储内置函数
//!
//! 设计要点：
//!   - 配置存储在用户目录下的 JSON 文件中（跨平台）
//!   - 路径：~/.sf/config.json（或 Windows %USERPROFILE%\.sf\config.json）
//!   - 首次调用自动创建目录和文件
//!   - 纯标准库实现，复用 jsonEncode/jsonDecode
//!
//! 函数列表：
//!   getCfgStr(key, default)     — 读取配置值，无则返回 default
//!   setCfgStr(key, value)      — 写入配置值
//!   removeCfgStr(key)          — 删除配置项
//!   getCfgStrAll()             — 返回所有配置（Map）

use std::sync::Mutex;

use crate::builtins_helpers as bh;
use crate::object_map::new_map;
use crate::value::{Value, error_value};
use crate::vm::VM;
use crate::function::BuiltinDoc;

static DOC_GETCFGSTR: BuiltinDoc = BuiltinDoc {
    category: "config",
    signature: "getCfgStr(key[, default]) -> string",
    summary: "读取配置项（持久化键值存储）。",
    params: &[("key", "配置键"), ("default", "可选。默认值")],
    returns: "string 配置值；不存在返回 default 或 undefined",
    examples: &["var v = getCfgStr(\"theme\")"],
    errors: &[],
};

static DOC_SETCFGSTR: BuiltinDoc = BuiltinDoc {
    category: "config",
    signature: "setCfgStr(key, val) -> undefined",
    summary: "写入配置项。",
    params: &[("key", "配置键"), ("val", "配置值")],
    returns: "undefined",
    examples: &["setCfgStr(\"theme\", \"dark\")"],
    errors: &[],
};

static DOC_REMOVECFGSTR: BuiltinDoc = BuiltinDoc {
    category: "config",
    signature: "removeCfgStr(key) -> undefined",
    summary: "删除配置项。",
    params: &[("key", "配置键")],
    returns: "undefined",
    examples: &[],
    errors: &[],
};

static DOC_GETCFGSTRALL: BuiltinDoc = BuiltinDoc {
    category: "config",
    signature: "getCfgStrAll() -> object",
    summary: "读取所有配置项。",
    params: &[],
    returns: "object 键值映射",
    examples: &[],
    errors: &[],
};

/// register 注册配置内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("getCfgStr", bi_get_cfg_str, &DOC_GETCFGSTR);
    vm.register_builtin_doc("setCfgStr", bi_set_cfg_str, &DOC_SETCFGSTR);
    vm.register_builtin_doc("removeCfgStr", bi_remove_cfg_str, &DOC_REMOVECFGSTR);
    vm.register_builtin_doc("getCfgStrAll", bi_get_cfg_all, &DOC_GETCFGSTRALL);
}

/// CONFIG 全局配置缓存（首次访问时从磁盘加载）。
static CONFIG: std::sync::OnceLock<Mutex<crate::ord_map::OrdMap>> = std::sync::OnceLock::new();

/// config_lock 获取全局配置的 Mutex 引用。
fn config_lock() -> &'static Mutex<crate::ord_map::OrdMap> {
    CONFIG.get_or_init(|| Mutex::new(load_config()))
}

/// config_path 返回配置文件路径。
///
/// 配置目录为用户主目录下的 `.sf`（与 `--cloud` 的 cloud.cfg 等共用同一目录）。
fn config_path() -> std::path::PathBuf {
    let home = dirs_home().unwrap_or_else(|| std::path::PathBuf::from("."));
    let cfg_dir = home.join(".sf");
    cfg_dir.join("config.json")
}

/// dirs_home 获取用户主目录（纯标准库）。
fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
}

/// load_config 从磁盘加载配置文件。
///
/// 健壮性：如果 JSON 解析失败（旧版本用 Value::to_str 写入的 "map{...}"
/// 调试格式），尝试自动修复（把 map{ 替换为 {）后重新解析。
fn load_config() -> crate::ord_map::OrdMap {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            // 第一次尝试：直接解析
            if let Some(om) = parse_json_to_ordmap(&content) {
                return om;
            }
            // 第二次尝试：兼容旧版本的 "map{...}" 调试格式
            // 旧版 save_config 用 Value::to_str 输出，嵌套 Map 显示为 map{...}
            let fixed: String = content.replace("map{", "{");
            if let Some(om) = parse_json_to_ordmap(&fixed) {
                // 自动修复成功，立即重写为合法 JSON
                let map = new_map();
                {
                    let mut guard = map.lock().unwrap();
                    for (k, v) in om.entries.iter() {
                        guard.set(k.clone(), v.clone());
                    }
                }
                let mut json = String::new();
                crate::builtins_json::encode_value(&Value::Object(map), &mut json);
                let _ = std::fs::write(&path, json);
                return om;
            }
            // 都失败：返回空配置（不报错，避免阻塞用户）
            crate::ord_map::OrdMap::new()
        }
        Err(_) => crate::ord_map::OrdMap::new(),
    }
}

/// parse_json_to_ordmap 把 JSON 字符串解析为 OrdMap。
/// 解析失败返回 None（不报错，让调用方决定如何处理）。
fn parse_json_to_ordmap(content: &str) -> Option<crate::ord_map::OrdMap> {
    let mut dec = crate::builtins_json::Decoder::new(content);
    match dec.parse_value() {
        Ok(Value::Object(m)) => {
            let guard = m.lock().unwrap();
            let mut om = crate::ord_map::OrdMap::new();
            for (k, v) in guard.data.iter() {
                om.set(k.clone(), v.clone());
            }
            Some(om)
        }
        Ok(Value::Map(m)) => {
            let guard = m.lock().unwrap();
            let mut om = crate::ord_map::OrdMap::new();
            for (k, v) in guard.entries.iter() {
                om.set(k.clone(), v.clone());
            }
            Some(om)
        }
        _ => None,
    }
}

/// save_config 保存配置到磁盘。
///
/// 重要：必须用 jsonEncode（encode_value）序列化，不能用 Value::to_str。
/// to_str 输出的是 Sflang 调试格式（嵌套 Map 显示为 "map{...}"），
/// 不是合法 JSON，load_config 解析会失败（整个配置读不到）。
fn save_config(cfg: &crate::ord_map::OrdMap) -> Result<(), Value> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let map = new_map();
    {
        let mut guard = map.lock().unwrap();
        for (k, v) in cfg.entries.iter() {
            guard.set(k.clone(), v.clone());
        }
    }
    // 用 JSON 编码器输出合法 JSON（避免调试格式的 "map{...}"）
    let mut json = String::new();
    crate::builtins_json::encode_value(&Value::Object(map), &mut json);
    std::fs::write(&path, json).map_err(|e| error_value(format!(
        "setCfgStr() 写入配置文件失败: {} (可能原因：目录无写权限或磁盘已满)", e,
    )))?;
    Ok(())
}

/// bi_get_cfg_str 读取配置值，无则返回 default。
fn bi_get_cfg_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let key = bh::as_str(args, 0, "getCfgStr")?;
    let cfg = config_lock();
    let guard = cfg.lock().unwrap();
    match guard.get(key) {
        Some(v) => Ok(v.clone()),
        None => Ok(args.get(1).cloned().unwrap_or(Value::Undefined)),
    }
}

/// bi_set_cfg_str 写入配置值并持久化。
fn bi_set_cfg_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let key = bh::as_str(args, 0, "setCfgStr")?.to_string();
    bh::require_arg(args, 1, "setCfgStr")?;
    let value = args[1].clone();
    let cfg = config_lock();
    let mut guard = cfg.lock().unwrap();
    guard.set(key, value);
    save_config(&guard)?;
    Ok(Value::Undefined)
}

/// bi_remove_cfg_str 删除配置项并持久化。
fn bi_remove_cfg_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let key = bh::as_str(args, 0, "removeCfgStr")?;
    let cfg = config_lock();
    let mut guard = cfg.lock().unwrap();
    let existed = guard.delete(key);
    if existed {
        save_config(&guard)?;
    }
    Ok(Value::Bool(existed))
}

/// bi_get_cfg_all 返回所有配置（Map）。
fn bi_get_cfg_all(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let cfg = config_lock();
    let guard = cfg.lock().unwrap();
    let map = new_map();
    {
        let mut m = map.lock().unwrap();
        for (k, v) in guard.entries.iter() {
            m.set(k.clone(), v.clone());
        }
    }
    Ok(Value::Object(map))
}
