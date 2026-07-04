//! object_map.rs — Sflang 的 Object/Map 类型实现
//!
//! 设计要点（来自 AGENTS.md）：
//!   - 基于 Rust HashMap 实现，附加原型链（Proto）支持轻量级面向对象
//!   - 运行时可以附加方法函数
//!   - 用 Mutex 实现内部可变性（配合 Arc 实现跨线程共享，阶段三）
//!   - 成员查找沿原型链向上
//!
//! 轻量级面向对象：通过 Proto 字段实现原型链，memberGet 沿链查找。
//! 方法以 Function 形式存放在 Map 中，调用时不自动绑定 this（建议用闭包捕获）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::value::Value;

/// Map 对象/映射类型，基于 HashMap，支持原型链。
///
/// 并发设计（阶段三）：用 Arc<Mutex<Map>> 共享。原型链查找（get_proto）
/// 采用安全迭代（沿链逐层 lock），删除了原有的 unsafe 指针操作。
pub struct Map {
    /// data 键值存储。
    pub data: HashMap<String, Value>,
    /// proto 原型指针（可为 None）。成员查找沿原型链向上。
    pub proto: Option<Arc<Mutex<Map>>>,
}

impl Map {
    /// new 创建一个空 Map。
    pub fn new() -> Self {
        Map {
            data: HashMap::new(),
            proto: None,
        }
    }

    /// with_proto 创建带原型的 Map。
    pub fn with_proto(proto: Arc<Mutex<Map>>) -> Self {
        Map {
            data: HashMap::new(),
            proto: Some(proto),
        }
    }

    /// get 读取键值（仅自身，不沿原型链）。
    pub fn get(&self, key: &str) -> Option<Value> {
        self.data.get(key).cloned()
    }

    /// get_proto 沿原型链读取键值。
    ///
    /// 先查自身，找不到则沿 proto 向上查找。
    /// 安全实现：沿链逐层 lock，用 Arc 指针地址做环检测（防原型链成环导致无限循环）。
    /// 不再使用 unsafe 指针解引用。
    pub fn get_proto(&self, key: &str) -> Option<Value> {
        // 先查自身
        if let Some(v) = self.data.get(key) {
            return Some(v.clone());
        }
        // 沿原型链向上，用指针地址做环检测
        let mut visited = std::collections::HashSet::new();
        let mut cur = self.proto.clone();
        while let Some(proto_arc) = cur {
            let addr = Arc::as_ptr(&proto_arc) as usize;
            if visited.contains(&addr) {
                break; // 环检测：已访问过，停止
            }
            visited.insert(addr);
            let guard = proto_arc.lock().unwrap();
            if let Some(v) = guard.data.get(key) {
                return Some(v.clone());
            }
            cur = guard.proto.clone();
            // guard 在此处 drop（离开作用域前手动释放）
            drop(guard);
        }
        None
    }

    /// set 设置键值（仅写入自身）。
    pub fn set(&mut self, key: String, val: Value) {
        self.data.insert(key, val);
    }

    /// has 判断是否包含键（仅自身）。
    pub fn has(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    /// delete 删除键。返回是否实际删除。
    pub fn delete(&mut self, key: &str) -> bool {
        self.data.remove(key).is_some()
    }

    /// keys 返回所有键（无序）。
    pub fn keys(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }

    /// len 返回自身键数量。
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// is_empty 判断是否为空（仅自身）。
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// iter 返回键值迭代器。
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.data.iter()
    }

    /// snapshot 克隆所有键值对为 Vec（用于持锁期间快速取数据后释放锁再处理）。
    ///
    /// 并发安全：在 inspect/equals 等递归函数中，先调用本方法取得快照，
    /// 然后释放锁，再对快照递归——避免持锁访问嵌套容器导致死锁。
    pub fn snapshot(&self) -> Vec<(String, Value)> {
        self.data.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
}

impl Default for Map {
    fn default() -> Self {
        Self::new()
    }
}

/// new_map 创建 Arc<Mutex<Map>>，方便使用。
pub fn new_map() -> Arc<Mutex<Map>> {
    Arc::new(Mutex::new(Map::new()))
}

/// new_map_with_proto 创建带原型的 Arc<Mutex<Map>>。
pub fn new_map_with_proto(proto: Arc<Mutex<Map>>) -> Arc<Mutex<Map>> {
    Arc::new(Mutex::new(Map::with_proto(proto)))
}
