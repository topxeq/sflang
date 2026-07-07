//! ring.rs — 通用环形缓冲区 Ring
//!
//! 设计对标 Charlang/tkc 的 AnyQueue + StringRing + ByteQueue 三者。
//! 用一个通用类型（持有 Value）替代三个特化类型。
//!
//! 底层用 VecDeque<Value>（环形数组），O(1) 头尾操作，内存连续。
//!
//! 容量语义：
//!   - cap > 0：固定容量，Push 超容量时自动淘汰头部最老的（FIFO 环形）
//!   - cap <= 0：无容量限制（动态扩容）
//!
//! 线程安全：通过 Arc<Mutex<Ring>> 共享，内置函数内部加锁。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::value::Value;

/// Ring 通用环形缓冲区。
///
/// 内部用 VecDeque 实现（环形数组），cap 为 0 或负数表示无限制。
/// cap > 0 时，Push 满则淘汰头部（最老的元素）。
pub struct Ring {
    /// buf 数据存储（环形数组）。
    pub buf: VecDeque<Value>,
    /// cap 容量上限。0 或负数表示无限制。
    pub cap: i64,
}

impl Ring {
    /// new 创建一个指定容量的 Ring。
    ///
    /// cap <= 0 表示无限制。
    pub fn new(cap: i64) -> Self {
        let initial = if cap > 0 { cap as usize } else { 0 };
        Ring {
            buf: VecDeque::with_capacity(initial),
            cap,
        }
    }

    /// size 返回当前元素数量。
    pub fn size(&self) -> usize {
        self.buf.len()
    }

    /// clear 清空所有元素。
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// push 尾部追加一个元素。
    ///
    /// 如果达到容量上限，先淘汰头部最老的元素。
    pub fn push(&mut self, v: Value) {
        if self.cap > 0 && self.buf.len() >= self.cap as usize {
            self.buf.pop_front();
        }
        self.buf.push_back(v);
    }

    /// get 查看指定位置的元素（不删除）。
    ///
    /// - 无索引或 0：取头部
    /// - -1：取尾部
    /// - 越界：返回 None
    pub fn get(&self, idx: i64) -> Option<Value> {
        let len = self.buf.len() as i64;
        if len == 0 {
            return None;
        }
        let real_idx = if idx < 0 { len - 1 } else { idx };
        if real_idx < 0 || real_idx >= len {
            return None;
        }
        self.buf.get(real_idx as usize).cloned()
    }

    /// set 修改指定位置的元素值。越界返回 false。
    pub fn set(&mut self, idx: i64, v: Value) -> bool {
        let len = self.buf.len() as i64;
        if idx < 0 || idx >= len {
            return false;
        }
        if let Some(slot) = self.buf.get_mut(idx as usize) {
            *slot = v;
            true
        } else {
            false
        }
    }

    /// pick 取出头部元素（删除）。空则返回 None。
    pub fn pick(&mut self) -> Option<Value> {
        self.buf.pop_front()
    }

    /// pop 取出尾部元素（删除）。空则返回 None。
    pub fn pop(&mut self) -> Option<Value> {
        self.buf.pop_back()
    }

    /// insert 在指定位置插入元素。
    ///
    /// 超容量时先淘汰尾部最新的。越界返回 false。
    pub fn insert(&mut self, idx: i64, v: Value) -> bool {
        let len = self.buf.len() as i64;
        if idx < 0 || idx > len {
            return false;
        }
        // 超容量淘汰尾部
        if self.cap > 0 && self.buf.len() >= self.cap as usize && self.buf.len() > 0 {
            self.buf.pop_back();
        }
        if idx as usize >= self.buf.len() {
            self.buf.push_back(v);
        } else {
            self.buf.insert(idx as usize, v);
        }
        true
    }

    /// remove 删除指定位置的元素。越界返回 false。
    pub fn remove(&mut self, idx: i64) -> bool {
        let len = self.buf.len() as i64;
        if idx < 0 || idx >= len {
            return false;
        }
        self.buf.remove(idx as usize).is_some()
    }

    /// to_list 转为 Vec<Value>（从头到尾顺序）。
    pub fn to_list(&self) -> Vec<Value> {
        self.buf.iter().cloned().collect()
    }
}

/// ring_value 将 Ring 包装为 Value::Native。
pub fn ring_value(ring: Ring) -> Value {
    Value::Native(Arc::new(Arc::new(Mutex::new(ring))))
}

/// ring_downcast 从 Value 中提取 Ring 引用。
///
/// 失败返回 AI 友好错误值。
pub fn ring_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<Mutex<Ring>>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<Mutex<Ring>>>().ok_or_else(|| {
            crate::value::error_value(format!(
                "{}() 参数不是 ring (可能原因：未用 newRing 创建，或传入了错误类型)",
                fn_name,
            ))
        }),
        Value::Undefined => Err(crate::value::error_value(format!(
            "{}() 参数为 undefined (可能原因：变量未初始化或函数返回了 undefined)",
            fn_name,
        ))),
        other => Err(crate::value::error_value(format!(
            "{}() 参数应为 ring，得到 {} (可能原因：参数顺序错误或未用 newRing 创建)",
            fn_name, other.type_name(),
        ))),
    }
}
