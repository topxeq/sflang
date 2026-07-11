//! builtins_cfg.rs — 持久化配置存储内置函数
//!
//! 设计要点：
//!   - 配置存储在用户目录下的 JSON 文件中（跨平台）
//!   - 路径：~/.sflang/config.json（或 Windows %USERPROFILE%\.sflang\config.json）
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

/// register 注册配置内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin("getCfgStr", bi_get_cfg_str);
    vm.register_builtin("setCfgStr", bi_set_cfg_str);
    vm.register_builtin("removeCfgStr", bi_remove_cfg_str);
    vm.register_builtin("getCfgStrAll", bi_get_cfg_all);
}

/// CONFIG 全局配置缓存（首次访问时从磁盘加载）。
static CONFIG: std::sync::OnceLock<Mutex<crate::ord_map::OrdMap>> = std::sync::OnceLock::new();

/// config_lock 获取全局配置的 Mutex 引用。
fn config_lock() -> &'static Mutex<crate::ord_map::OrdMap> {
    CONFIG.get_or_init(|| Mutex::new(load_config()))
}

/// config_path 返回配置文件路径。
fn config_path() -> std::path::PathBuf {
    let home = dirs_home().unwrap_or_else(|| std::path::PathBuf::from("."));
    let cfg_dir = home.join(".sflang");
    cfg_dir.join("config.json")
}

/// dirs_home 获取用户主目录（纯标准库）。
fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
}

/// load_config 从磁盘加载配置文件。
fn load_config() -> crate::ord_map::OrdMap {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let mut dec = crate::builtins_json::Decoder::new(&content);
            match dec.parse_value() {
                Ok(Value::Object(m)) => {
                    let guard = m.lock().unwrap();
                    let mut om = crate::ord_map::OrdMap::new();
                    for (k, v) in guard.data.iter() {
                        om.set(k.clone(), v.clone());
                    }
                    om
                }
                Ok(Value::Map(m)) => {
                    let guard = m.lock().unwrap();
                    let mut om = crate::ord_map::OrdMap::new();
                    for (k, v) in guard.entries.iter() {
                        om.set(k.clone(), v.clone());
                    }
                    om
                }
                _ => crate::ord_map::OrdMap::new(),
            }
        }
        Err(_) => crate::ord_map::OrdMap::new(),
    }
}

/// save_config 保存配置到磁盘。
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
    let json = Value::Object(map).to_str();
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
