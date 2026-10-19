//! clock.rs — 平台时钟抽象。
//!
//! wasm32-unknown-unknown 上 std::time::Instant::now / SystemTime::now 未实现
//! （调用即 panic，见 std sys/time/unsupported.rs）。浏览器环境经 env.js_now 导入
//! 取 JS Date.now() 毫秒；桌面环境用标准库真单调时钟。
//!
//! 使用规则：调度器定时器/睡眠截止用 Mono（单调语义）；Unix 毫秒时间戳用
//! now_millis()。新代码不要直接调 std::time::*::now()，一律经本模块。

/// js_now JS 侧导入：返回 Date.now()（Unix 毫秒，f64）。
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    fn js_now() -> f64;
}

/// now_millis 当前 Unix 时间毫秒数。
/// 桌面：SystemTime；wasm：JS Date.now()。
pub fn now_millis() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    unsafe {
        js_now() as u64
    }
}

/// Mono 单调时钟点（调度器定时器、睡眠截止用）。
/// 桌面 = std::time::Instant（真单调）；wasm = Unix 毫秒（Date.now，对
/// Playground 级定时精度足够；非严格单调的场景如系统回拨不影响演示用途）。
#[cfg(not(target_arch = "wasm32"))]
pub type Mono = std::time::Instant;

#[cfg(target_arch = "wasm32")]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mono(u64);

/// mono_now 当前单调时钟点。
#[cfg(not(target_arch = "wasm32"))]
pub fn mono_now() -> Mono {
    std::time::Instant::now()
}

/// mono_now 当前单调时钟点（wasm：取 JS 时间）。
#[cfg(target_arch = "wasm32")]
pub fn mono_now() -> Mono {
    Mono(now_millis())
}

#[cfg(target_arch = "wasm32")]
impl Mono {
    /// saturating_duration_since 到较早时点的时间差（饱和为 0）。
    pub fn saturating_duration_since(self, earlier: Mono) -> std::time::Duration {
        std::time::Duration::from_millis(self.0.saturating_sub(earlier.0))
    }
    /// elapsed 自此时点起经过的时长。
    pub fn elapsed(self) -> std::time::Duration {
        std::time::Duration::from_millis(now_millis().saturating_sub(self.0))
    }
}

#[cfg(target_arch = "wasm32")]
impl std::ops::Add<std::time::Duration> for Mono {
    type Output = Mono;
    fn add(self, d: std::time::Duration) -> Mono {
        Mono(self.0.wrapping_add(d.as_millis() as u64))
    }
}
