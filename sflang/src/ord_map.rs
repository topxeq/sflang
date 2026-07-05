//! ord_map.rs — 有序映射类型（插入顺序保持，纯数据容器）
//!
//! 设计要点：
//!   - 内部用 Vec<(String, Value)> 保持插入顺序
//!   - 纯数据容器：无原型链、不挂方法
//!   - 与 Object 区分：Object 是 OOP 载体（HashMap 无序 + 原型链），Map 是有序数据映射
//!   - 查找 O(n) 线性（Map 通常不大，可接受）

use crate::value::Value;

/// OrdMap 有序映射，基于 Vec<(String, Value)>。
pub struct OrdMap {
    /// entries 键值对（插入顺序）。
    pub entries: Vec<(String, Value)>,
}

impl OrdMap {
    /// new 创建空 OrdMap。
    pub fn new() -> Self {
        OrdMap { entries: Vec::new() }
    }

    /// get 读取键值（线性查找）。
    pub fn get(&self, key: &str) -> Option<Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    /// set 设置键值（已存在则更新，不存在则追加）。
    pub fn set(&mut self, key: String, val: Value) {
        for (k, v) in &mut self.entries {
            if *k == key {
                *v = val;
                return;
            }
        }
        self.entries.push((key, val));
    }

    /// has 判断是否包含键。
    pub fn has(&self, key: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    /// delete 删除键，返回是否删除。
    pub fn delete(&mut self, key: &str) -> bool {
        if let Some(pos) = self.entries.iter().position(|(k, _)| k == key) {
            self.entries.remove(pos);
            true
        } else {
            false
        }
    }

    /// keys 返回所有键（插入顺序）。
    pub fn keys(&self) -> Vec<String> {
        self.entries.iter().map(|(k, _)| k.clone()).collect()
    }

    /// values 返回所有值（插入顺序）。
    pub fn values(&self) -> Vec<Value> {
        self.entries.iter().map(|(_, v)| v.clone()).collect()
    }

    /// len 返回键值对数量。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// is_empty 判断是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// snapshot 克隆所有键值对（用于持锁期间快速取数据后释放锁）。
    pub fn snapshot(&self) -> Vec<(String, Value)> {
        self.entries.clone()
    }
}

impl Default for OrdMap {
    fn default() -> Self {
        Self::new()
    }
}

/// new_ord_map 创建 Arc<Mutex<OrdMap>>。
pub fn new_ord_map() -> std::sync::Arc<std::sync::Mutex<OrdMap>> {
    std::sync::Arc::new(std::sync::Mutex::new(OrdMap::new()))
}
