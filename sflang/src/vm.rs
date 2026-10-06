//! vm.rs — 字节码虚拟机
//!
//! 设计要点：
//!   - 基于栈的 VM，递归式函数调用（每帧独立 locals 数组）
//!   - try/catch/finally：三阶段状态机（Body/Catch/Finally），try 入口常驻
//!     try 栈直到语句结束；throw/return/break/continue 穿越时挂起到入口的
//!     pending，先进 finally，ExitFinally 恢复——保证 finally 必然执行
//!   - 控制流事件（return/throw/跳转穿越）在同一 run_frame 循环内处置，
//!     不再递归重入，深循环中的 try-catch 不会累积 Rust 栈帧
//!   - defer：注册到当前帧，任何退出路径（正常 return 或异常穿透）都逆序执行
//!   - run 关键字：启动新线程（共享全局，独立 VM 状态）
//!   - 局部变量用 slot 数组，闭包用 box 共享

use std::sync::{Arc, Mutex};

use crate::compiler::compile;
use crate::function::{Builtin, Function};
use crate::lexer::tokenize;
use crate::opcode::{Code, Opcode};
use crate::parser::parse_program;
use crate::scheduler;
use crate::value::{error_value, Value, SfError};

/// TASK_SLICE_FUEL 单个任务切片的指令预算。
///
/// 每个调度切片最多执行这么多条指令后让出（协作式抢占）。
/// 取值权衡：太大→长任务挤占其他任务；太小→帧压回/恢复的开销占比上升。
/// 10 万条指令约对应亚毫秒级切片，公平性与开销均衡。
pub(crate) const TASK_SLICE_FUEL: u64 = 100_000;

/// flow_kind 控制流类型。
#[derive(Clone, Copy, PartialEq, Eq)]
enum FlowKind {
    /// Normal 正常执行（无控制流跳转）。
    Normal,
    /// Return 返回（值在 FlowResult.value）。
    Return,
    /// Throw 抛出异常（值在 FlowResult.value）。
    Throw,
    /// Yield 任务切片让出（燃料耗尽或内置函数挂起任务）。仅在任务切片执行
    /// （run_task_slice，燃料有限）中出现；非任务上下文燃料无限制，不会产生。
    Yield,
}

/// flow_result 帧执行结果。
struct FlowResult {
    /// value 控制流携带的值（返回值或异常值）。
    value: Value,
    /// kind 控制流类型。
    kind: FlowKind,
}

/// try_phase try 语句的阶段状态。
///
/// 入口从 PushTry 开始常驻 try 栈，直到整条 try 语句（含 finally）结束才弹出。
/// 阶段标记用于区分异常/控制流发生在 body、catch 还是 finally 中：
///   - body 中异常 → 有 catch 进 catch，否则挂起进 finally
///   - catch 中异常 → 挂起进 finally（不再进本 catch）
///   - finally 中的异常/return → 直接弹出本入口向外传播（覆盖挂起值）
#[derive(Clone, Copy, PartialEq, Eq)]
enum TryPhase {
    /// Body 正在执行 try 块。
    Body,
    /// Catch 正在执行 catch 块。
    Catch,
    /// Finally 正在执行 finally 块。
    Finally,
}

/// 无对应块的 ip 哨兵值（PushTry 的 u16 操作数 0xFFFF 转换而来）。
const NO_IP: usize = usize::MAX;

/// try_entry try 上下文（编译期 PushTry 创建，常驻至语句结束）。
struct TryEntry {
    /// catch_ip catch 块入口（NO_IP 表示无 catch）。
    catch_ip: usize,
    /// finally_ip finally 块入口（NO_IP 表示无 finally）。
    finally_ip: usize,
    /// end_ip 整条 try 语句之后的地址（finally 正常完成继续执行处）。
    end_ip: usize,
    /// phase 当前阶段。
    phase: TryPhase,
    /// snapshot PushTry 时的操作数栈深度（进入 catch/finally 前回退，清理半求值表达式）。
    snapshot: usize,
    /// pending finally 期间挂起的控制流（None 表示正常完成）。
    pending: Option<PendingFlow>,
}

/// pending_flow finally 期间挂起的控制流。
///
/// body/catch 中出现 return/throw/break/continue 且存在 finally 时，
/// 控制流挂起到入口，先执行 finally；ExitFinally 时恢复。
/// finally 自身若产生 return/throw，则覆盖（丢弃）挂起值。
enum PendingFlow {
    /// Return 挂起的函数返回（值为返回值）。
    Return(Value),
    /// Throw 挂起的异常（值为异常值）。
    Throw(Value),
    /// Jump 挂起的 break/continue 跳转（target 为目标地址，leave 为剩余待穿越 try 层数）。
    Jump { target: usize, leave: u8 },
}

/// event 帧内控制流事件（在 run_frame 主循环内处置，不递归重入）。
enum Event {
    /// Return 函数返回（值为返回值）。
    Return(Value),
    /// Throw 异常（值为异常值）。
    Throw(Value),
    /// Jump break/continue 跳转穿越 try（target 目标地址，leave 待穿越层数）。
    Jump { target: usize, leave: u8 },
}

/// dispatch_outcome 控制流事件处置结果。
enum DispatchOutcome {
    /// Continue 已设置 frame.ip，继续执行本帧指令循环。
    Continue,
    /// Done 帧结束（defers 已执行），携带最终结果。
    Done(FlowResult),
}

/// defer_entry defer 调用。
struct DeferEntry {
    /// callee 被调用的函数。
    callee: Value,
    /// args 实参列表。
    args: Vec<Value>,
}

/// FinishState 帧收尾状态：帧逻辑结束后、defers 迭代执行期间的暂存信息。
///
/// 迭代化后 defers 不再内联递归执行，而是经帧栈逐个发起调用：
/// 帧以"收尾中"状态留在栈上，每个 defer 调用作为普通被调帧执行，
/// 其结果记回本状态（错误覆盖语义与原递归版一致）。
struct FinishState {
    /// result 帧的原始结束结果（Return 或 Throw；defer 错误可能覆盖为 Throw）。
    result: FlowResult,
    /// remaining_defers 待执行的 defer（逆序弹出执行）。
    remaining_defers: Vec<DeferEntry>,
    /// defer_err 已发生的 defer 错误（后执行的覆盖先前的）。
    defer_err: Option<Value>,
}

/// Resume 帧结束结果的交付协议（帧创建时确定，不变）。
///
/// 决定帧最终结束（defers 全部执行完）时结果去向：
#[derive(Clone, Copy, PartialEq)]
enum Resume {
    /// TopLevel 顶层帧：结果作为 execute_frames 的返回值交给 Rust 调用方
    /// （run() 顶层执行、import 子脚本、call_function_value 发起的调用）。
    TopLevel,
    /// PushResult 调用返回：结果交付给调用方帧——
    /// Return 值压回操作数栈；Throw 沿调用方 try 栈传播（可被 catch/finally 接住）。
    PushResult,
}

/// CallErr start_call 的失败/切换结果。
///
/// 与 Result<Value, Value> 区分：EnterFrame 不是错误，而是"已生成被调帧、
/// 需交回机器循环"的迭代式调用信号。
enum CallErr {
    /// Thrown 调用失败（不可调用/超最大深度/内置函数抛错），携带异常值。
    Thrown(Value),
    /// EnterFrame 被调方是用户函数，携带其新帧——调用方先压回自身帧（ip 已
    /// 推进过调用指令），再压入此帧，返回机器循环。
    EnterFrame(Frame),
    /// Parked 等待-重试型内置函数挂起了任务：callee 与实参已回推到操作数栈，
    /// 调用点须回退 ip 到本调用指令并让出切片（唤醒后重新执行）。
    Parked,
}

/// Frame 调用帧。
struct Frame {
    /// code 本帧执行的字节码。
    code: Arc<Code>,
    /// ip 指令指针。
    ip: usize,
    /// locals 局部变量数组（含参数）。
    locals: Vec<Value>,
    /// boxes 被捕获的 local（box 共享）。惰性分配。
    boxes: std::collections::HashMap<usize, Arc<Mutex<Value>>>,
    /// free_vars 闭包捕获的自由变量（box 共享，跨线程可变）。
    free_vars: Vec<Arc<Mutex<Value>>>,
    /// defers 已注册的 defer 调用（按注册顺序，帧退出时逆序执行）。
    defers: Vec<DeferEntry>,
    /// try_stack try 上下文栈。
    try_stack: Vec<TryEntry>,
    /// resume 帧结束结果的交付协议（创建时确定；见 Resume）。
    resume: Resume,
    /// finish 收尾状态：帧逻辑结束后进入（defers 迭代执行期间）；None 表示执行中。
    finish: Option<FinishState>,
}

impl Frame {
    fn new(code: Arc<Code>, free_vars: Vec<Arc<Mutex<Value>>>) -> Self {
        let locals = vec![Value::Undefined; code.num_locals];
        Frame {
            code,
            ip: 0,
            locals,
            boxes: std::collections::HashMap::new(),
            free_vars,
            defers: Vec::new(),
            try_stack: Vec::new(),
            resume: Resume::TopLevel,
            finish: None,
        }
    }
}

/// 全局核心内置函数表（进程内仅构建一次，所有 VM 共享只读引用）。
///
/// 背景：此前每个 VM（包括 run/runAsync 启动的每个子线程 VM）都要重新注册
/// 700+ 个内置函数，每次耗时数微秒、占约几十 KB 内存。改为进程级 OnceLock
/// 静态表后，VM::new 只需共享引用，子线程 VM 的创建成本大幅下降。
/// 每个 VM 仍可注册自定义内置函数（extra_builtins，查找优先于核心表），
/// 保持嵌入式 API 可扩展、可覆盖的既有语义。
static CORE_BUILTINS: std::sync::OnceLock<std::collections::HashMap<&'static str, Builtin>> =
    std::sync::OnceLock::new();

/// core_builtins 获取全局核心内置函数表（首次访问时构建）。
///
/// 构建方式：用裸 VM（new_raw，不注册任何函数）作暂存区执行全部核心模块
/// 注册，再把表抽走。Builtin.name 本身是 &'static str，直接作键，
/// 无分配无泄漏。OnceLock 保证并发首访只构建一次。
fn core_builtins() -> &'static std::collections::HashMap<&'static str, Builtin> {
    CORE_BUILTINS.get_or_init(|| {
        let mut vm = VM::new_raw();
        register_all_core(&mut vm);
        vm.extra_builtins.into_values().map(|b| (b.name, b)).collect()
    })
}

/// register_all_core 注册全部核心内置函数模块（构建全局表用，进程内仅执行一次）。
///
/// 模块清单与历史 VM::new 完全一致（核心/字符串/数学/数组/时间/文件/JSON/
/// 并发/GUI 等），保证内置函数集不变。
fn register_all_core(vm: &mut VM) {
    crate::builtins::register(vm);
    crate::builtins_str::register(vm);
    crate::builtins_math::register(vm);
    crate::builtins_arr::register(vm);
    crate::builtins_time::register(vm);
    crate::builtins_fs::register(vm);
    crate::builtins_json::register(vm);
    crate::builtins_bytes::register(vm);
    crate::builtins_bigint::register(vm);
    crate::builtins_regex::register(vm);
    crate::builtins_encode::register(vm);
    crate::builtins_hash::register(vm);
    crate::builtins_sys::register(vm);
    crate::concurrency::register(vm);
    crate::builtins_ring::register(vm);
    crate::builtins_csv::register(vm);
    crate::builtins_xlsx::register(vm);
    crate::builtins_docx::register(vm);
    crate::builtins_db::register(vm);
    crate::builtins_aes::register(vm);
    crate::txde::register(vm);
    // GUI 仅 Windows 提供；其他平台注册桩函数（调用返回明确的平台不支持错误）
    #[cfg(all(feature = "gui", target_os = "windows"))]
    crate::builtins_gui::register(vm);
    #[cfg(all(feature = "gui", not(target_os = "windows")))]
    crate::builtins_gui_stub::register(vm);
    crate::builtins_ssh::register(vm);
    crate::builtins_le::register(vm);
    crate::builtins_email::register(vm);
    crate::builtins_ftp::register(vm);
    crate::builtins_http::register(vm);
    crate::builtins_zip::register(vm);
    crate::builtins_containers::register(vm);
    crate::builtins_xml::register(vm);
    crate::builtins_clipboard::register(vm);
    crate::builtins_dialog::register(vm);
    crate::builtins_async::register(vm);
    crate::builtins_test::register(vm);
    crate::builtins_pinyin::register(vm);
    crate::builtins_jwt::register(vm);
    crate::builtins_rsa::register(vm);
    crate::builtins_cfg::register(vm);
    crate::builtins_template::register(vm);
    crate::builtins_tcp::register(vm);
    crate::builtins_proxy::register(vm);
    crate::builtins_xxci::register(vm);
    crate::builtins_image::register(vm);
    crate::builtins_image_gen::register(vm);
    crate::builtins_seq::register(vm);
    crate::builtins_s3::register(vm);
}

/// pause 任务切片暂停原因（切片执行中由内置函数/燃料机制置位）。
///
/// 两种挂起模型（start_call 与调用点按此分派唤醒后的续接方式）：
///   - Parked（等待-重试型）：唤醒后回退到调用指令重新执行（重查条件再获取）。
///     适用于资源类等待：lock/rlock/wlock/wgWait/semAcquire。
///   - ParkedInject（值注入型）：唤醒后从调用指令之后继续，结果位占位
///     undefined 被注入值替换。适用于值/完成类等待：chanRecv/onceDo 等待者/sleep。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pause {
    /// None 未暂停（正常执行）。
    None,
    /// Parked 等待-重试型挂起（唤醒后重试调用）。
    Parked,
    /// ParkedInject 值注入型挂起（唤醒后从调用后继续，占位值被替换）。
    ParkedInject,
    /// YieldFuel 燃料耗尽让出（可再次调度）。
    YieldFuel,
}

/// VM 虚拟机。
pub struct VM {
    /// stack 操作数栈。
    stack: Vec<Value>,
    /// frames 调用帧栈（迭代式执行：函数调用压帧而非递归，任务状态可整体保存/恢复）。
    frames: Vec<Frame>,
    /// globals 全局变量（跨线程共享，run 启动的线程与本 VM 共享同一份）。
    globals: Arc<Mutex<std::collections::HashMap<String, Value>>>,
    /// extra_builtins 本 VM 额外注册的自定义内置函数（覆盖全局核心表同名函数）。
    /// 通常为空（核心函数在全局表 CORE_BUILTINS 中，进程内共享）。
    extra_builtins: std::collections::HashMap<String, Builtin>,
    /// out 标准输出（跨线程共享）。
    out: Arc<Mutex<dyn std::io::Write + Send>>,
    /// max_call_depth 最大调用深度。
    max_call_depth: usize,
    /// depth 当前调用深度。
    depth: usize,
    /// fuel 指令燃料：任务切片预算，每执行一条指令扣 1，耗尽让出切片。
    /// 非任务上下文（主脚本/poolRun 工作线程等）为 u64::MAX，不受限。
    fuel: u64,
    /// pause 切片暂停原因（None=正常执行；任务切片执行中可能被置位）。
    pause: Pause,
    /// park_disabled 挂起禁用计数（>0 时挂起类操作返回错误而非挂起）。
    /// call_function_value 执行回调期间递增：回调内挂起会破坏调用方内置函数
    /// （sort/onceDo 等）的 Rust 栈上状态，故禁止并给出明确错误引导。
    park_disabled: u32,
    /// prepared_result 任务体是内置函数时的预执行结果（prepare_task_call 用）。
    prepared_result: Option<Result<Value, Value>>,
    /// pending_throw 待投递的异常（阻塞池卸载的内置函数失败时经调度器写入，
    /// 任务恢复执行的切片开始时在栈顶帧上抛出——调用点的 try/catch 可捕获）。
    pending_throw: Option<Value>,
    /// import_stack 正在加载的脚本绝对路径栈（环检测，防循环 import）。
    import_stack: Vec<String>,
    /// imported_modules 已成功加载的模块规范化路径（模块缓存，保证幂等）。
    imported_modules: Vec<String>,
}

impl VM {
    /// new 创建虚拟机（内置函数取自全局核心表，进程内仅注册一次）。
    ///
    /// 在此统一预置各 VM 实例私有的部分（预定义数学常量等全局变量）；
    /// 核心内置函数表由 core_builtins() 保证进程内仅构建一次。
    pub fn new() -> Self {
        // 确保全局核心内置函数表已构建（并发首访时仅一次）
        let _ = core_builtins();
        let mut vm = VM::new_raw();
        // 预定义数学常量全局变量（globals 为每 VM 私有，不能进全局函数表）
        vm.set_global("piG", Value::Float(std::f64::consts::PI));
        vm.set_global("eG", Value::Float(std::f64::consts::E));
        vm
    }

    /// new_raw 创建裸 VM（不注册核心内置函数、不预置全局变量）。
    ///
    /// 仅供 core_builtins() 构建全局表时作暂存区使用，外部应使用 new()。
    fn new_raw() -> Self {
        VM {
            // 初始容量取小值按需增长：每 VM 预分配会直接乘到任务内存上
            // （10 万任务 × 1024×sizeof(Value) ≈ 数 GB；64 容量 ≈ 1.5KB/任务）
            stack: Vec::with_capacity(64),
            frames: Vec::with_capacity(8),
            globals: Arc::new(Mutex::new(std::collections::HashMap::new())),
            extra_builtins: std::collections::HashMap::new(),
            out: Arc::new(Mutex::new(std::io::sink())),
            // max_call_depth 限制递归深度。
            // 迭代化后脚本调用链占用堆上的帧栈，max_call_depth 只约束逻辑深度；
            // Rust 侧递归仅剩"内置→脚本"嵌套（call_function_value），同样受此约束。
            max_call_depth: 500,
            depth: 0,
            // fuel 任务切片燃料：非任务执行不受限（u64::MAX）
            fuel: u64::MAX,
            pause: Pause::None,
            park_disabled: 0,
            prepared_result: None,
            pending_throw: None,
            import_stack: Vec::new(),
            imported_modules: Vec::new(),
        }
    }

    /// set_output 设置标准输出（须 Send 以支持跨线程共享）。
    pub fn set_output(&mut self, w: impl std::io::Write + Send + 'static) {
        self.out = Arc::new(Mutex::new(w));
    }

    /// set_output_handle 直接设置 Arc<Mutex<dyn Write + Send>> 句柄（用于线程间共享）。
    pub fn set_output_handle(&mut self, out: Arc<Mutex<dyn std::io::Write + Send>>) {
        self.out = out;
    }

    /// output_handle 获取输出句柄（供内置函数使用）。
    pub fn output_handle(&self) -> Arc<Mutex<dyn std::io::Write + Send>> {
        self.out.clone()
    }

    /// set_global 设置全局变量（线程安全，加锁）。
    pub fn set_global(&mut self, name: &str, val: Value) {
        self.globals.lock().unwrap().insert(name.to_string(), val);
    }

    /// get_global 读取全局变量（线程安全，加锁）。
    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.globals.lock().unwrap().get(name).cloned()
    }

    /// register_builtin 注册自定义内置函数（本 VM 私有，覆盖全局核心表同名函数）。
    ///
    /// 核心内置函数在进程级全局表中（core_builtins，进程内仅注册一次），
    /// 此方法用于嵌入式场景追加自定义函数。
    pub fn register_builtin(&mut self, name: &'static str, func: crate::function::BuiltinFn) {
        self.extra_builtins.insert(name.to_string(), Builtin::new(name, func));
    }

    /// register_builtin_doc 注册带文档的自定义内置函数（用于 help 系统）。
    /// 覆盖语义同 register_builtin。
    pub fn register_builtin_doc(
        &mut self,
        name: &'static str,
        func: crate::function::BuiltinFn,
        doc: &'static crate::function::BuiltinDoc,
    ) {
        self.extra_builtins
            .insert(name.to_string(), Builtin::new_with_doc(name, func, doc));
    }

    /// register_builtin_doc_blocking 注册阻塞型内置函数（核心模块专用，进全局表）。
    ///
    /// 阻塞型 = 网络/文件 IO 等慢操作：任务上下文调用时卸载到阻塞线程池执行，
    /// 任务挂起等待结果（不占调度 worker）。标记前提见 Builtin.blocking 文档。
    /// 注册目标为全局核心表（构建期使用），与 core_builtins() 的暂存 VM 配合。
    pub fn register_builtin_doc_blocking(
        &mut self,
        name: &'static str,
        func: crate::function::BuiltinFn,
        doc: &'static crate::function::BuiltinDoc,
    ) {
        self.extra_builtins
            .insert(name.to_string(), Builtin::new_blocking_with_doc(name, func, doc));
    }

    /// lookup_builtin_in 按名查找内置函数，返回克隆（Builtin 为 3 个字，克隆廉价）。
    ///
    /// 查找顺序：本 VM 自定义表（可覆盖）→ 全局核心表。
    /// 参数化为 extra 表引用：指令循环内以字段级借用调用（与帧借用不相交）。
    fn lookup_builtin_in(extra: &std::collections::HashMap<String, Builtin>, name: &str) -> Option<Builtin> {
        if let Some(b) = extra.get(name) {
            return Some(b.clone());
        }
        core_builtins().get(name).cloned()
    }

    /// builtin_names 返回所有内置函数名（按字母序，含自定义覆盖项）。
    /// 用于 help() 无参调用时列出全部函数。
    pub fn builtin_names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self
            .extra_builtins
            .values()
            .map(|b| b.name)
            .collect();
        names.extend(core_builtins().values().map(|b| b.name));
        names.sort();
        names.dedup();
        names
    }

    /// builtin_exists 判断内置函数是否存在（按名字，含自定义表）。
    pub fn builtin_exists(&self, name: &str) -> bool {
        self.extra_builtins.contains_key(name) || core_builtins().contains_key(name)
    }

    /// builtin_doc 查询某内置函数的文档元数据。
    /// 返回 None 表示函数不存在或暂无文档。
    pub fn builtin_doc(&self, name: &str) -> Option<&'static crate::function::BuiltinDoc> {
        if let Some(b) = self.extra_builtins.get(name) {
            return b.doc;
        }
        core_builtins().get(name).and_then(|b| b.doc)
    }

    /// builtin_categories 按分类聚合内置函数名。
    /// 返回 (分类, [函数名]) 列表，分类按字母序，函数名按字母序。
    /// 无文档的函数归入 "(uncategorized)" 分类。自定义表优先（同名覆盖核心表）。
    pub fn builtin_categories(&self) -> Vec<(&'static str, Vec<&'static str>)> {
        use std::collections::BTreeMap;
        // 用 BTreeMap 自动按分类名排序
        let mut by_cat: BTreeMap<&'static str, Vec<&'static str>> = BTreeMap::new();
        // 自定义表先入（同名者覆盖核心表，核心表阶段跳过）
        for b in self.extra_builtins.values() {
            let cat = b.doc.map(|d| d.category).unwrap_or("(uncategorized)");
            by_cat.entry(cat).or_default().push(b.name);
        }
        let core = core_builtins();
        for b in core.values() {
            if self.extra_builtins.contains_key(b.name) {
                continue; // 被本 VM 自定义函数覆盖
            }
            let cat = b.doc.map(|d| d.category).unwrap_or("(uncategorized)");
            by_cat.entry(cat).or_default().push(b.name);
        }
        // 每个分类内函数名去重并排序
        let mut result: Vec<(&'static str, Vec<&'static str>)> = Vec::new();
        for (cat, mut names) in by_cat {
            names.sort();
            names.dedup();
            result.push((cat, names));
        }
        result
    }

    /// globals_handle 获取全局变量的共享句柄（Arc<Mutex<HashMap>>）。
    ///
    /// 用于 run 启动子线程时共享同一份全局环境（而非克隆快照），
    /// 使主线程与子线程的 var/func 定义互通。
    pub fn globals_handle(&self) -> Arc<Mutex<std::collections::HashMap<String, Value>>> {
        self.globals.clone()
    }

    /// set_globals_handle 设置全局变量的共享句柄（用于子线程接入主线程的全局环境）。
    pub fn set_globals_handle(&mut self, globals: Arc<Mutex<std::collections::HashMap<String, Value>>>) {
        self.globals = globals;
    }

    /// run 执行顶层 Code。
    ///
    /// 迭代化后：压入 TopLevel 帧交给 execute_frames 驱动。
    /// 可重入：import 在机器执行中途调用本方法时，子脚本帧压在现有帧栈之上，
    /// 只运行到该帧结束，不影响下方暂停中的调用链。
    pub fn run(&mut self, code: Arc<Code>) -> Result<Value, Value> {
        let frame = Frame::new(code, Vec::new()); // resume 默认 TopLevel
        self.frames.push(frame);
        // 本入口不受任务切片燃料约束（import 子脚本等在任务内执行时，
        // 模块代码作为整体跑完，不被预算抢占打断；主脚本入口本就无燃料限制）
        let saved_fuel = self.fuel;
        self.fuel = u64::MAX;
        let res = self.execute_frames();
        self.fuel = saved_fuel;
        match res.kind {
            FlowKind::Throw => Err(res.value),
            // Yield 理论上不可达（燃料无限制）；防御性按正常结束处理
            _ => Ok(res.value),
        }
    }

    /// call_function_value 调用一个函数值（Func 或 Builtin），返回其结果。
    ///
    /// 供内置函数调用用户函数（如 onceDo 执行一次性回调、sort 自定义比较器等）。
    /// 用户函数经帧栈迭代执行（TopLevel 帧），错误转为 Result；
    /// Rust 侧递归深度只随"内置→脚本"嵌套层数增长（受 max_call_depth 约束），
    /// 不随脚本调用深度增长。
    ///
    /// 燃料语义：回调执行不受任务切片预算约束（恢复无限燃料）。当前限制：
    /// 内置函数内部的原生循环（如 sort 遍历）与回调整体不可被预算抢占。
    pub fn call_function_value(&mut self, callee: Value, args: Vec<Value>) -> Result<Value, Value> {
        match callee {
            Value::Builtin(b) => (b.func)(self, &args),
            Value::Func(f) => {
                if self.depth >= self.max_call_depth {
                    return Err(error_value(format!(
                        "max call depth exceeded ({}); 可能原因：递归过深", self.max_call_depth
                    )));
                }
                self.depth += 1;
                let mut new_frame = Frame::new(f.body.clone(), f.free_vars.clone());
                self.bind_params(&f, &args, &mut new_frame);
                new_frame.resume = Resume::TopLevel;
                self.frames.push(new_frame);
                let saved_fuel = self.fuel;
                self.fuel = u64::MAX;
                // 回调执行期间禁止挂起：回调内挂起会把"已挂起"状态泄漏给调用方
                // 内置函数（sort/onceDo 等无法恢复其 Rust 栈上进度），产生状态错乱。
                // 挂起类操作在回调内返回明确错误，引导脚本把等待移出回调。
                self.push_park_disabled();
                let res = self.execute_frames();
                self.pop_park_disabled();
                self.fuel = saved_fuel;
                self.depth -= 1;
                match res.kind {
                    FlowKind::Throw => Err(res.value),
                    // Yield 理论上不可达（回调燃料无限制且挂起被禁）；防御性按正常结束处理
                    _ => Ok(res.value),
                }
            }
            Value::Undefined => Err(error_value(
                "not callable: undefined (可能原因：调用了未定义的函数名；请检查函数是否已定义、拼写是否正确；内置函数可用 help(分类) 查询，未定义变量可用 explainUndef(\"名字\") 诊断)",
            )),
            other => Err(error_value(format!(
                "not callable: {} (可能原因：调用了非函数值；请检查变量是否为函数)", other.type_name()
            ))),
        }
    }

    // ---- 任务切片执行接口（供调度器 scheduler.rs 使用；阶段二） ----

    /// prepare_task_call 任务首帧准备：等价 call_function_value 的压帧部分，
    /// 但不执行——执行由调度器驱动 run_task_slice 逐切片进行。
    ///
    /// 任务体是内置函数时直接预执行（内置函数无挂起意义的首帧），
    /// 结果暂存 prepared_result，首次 run_task_slice 直接返回。
    pub(crate) fn prepare_task_call(&mut self, callee: Value, args: Vec<Value>) -> Result<(), Value> {
        match callee {
            Value::Func(f) => {
                let mut new_frame = Frame::new(f.body.clone(), f.free_vars.clone());
                self.bind_params(&f, &args, &mut new_frame);
                new_frame.resume = Resume::TopLevel;
                self.frames.push(new_frame);
                Ok(())
            }
            Value::Builtin(b) => {
                // 内置函数任务体：预执行并暂存结果
                self.prepared_result = Some((b.func)(self, &args));
                Ok(())
            }
            Value::Undefined => Err(error_value(
                "not callable: undefined (可能原因：run 的目标函数未定义；请检查函数是否已定义、拼写是否正确)",
            )),
            other => Err(error_value(format!(
                "not callable: {} (可能原因：run 的目标不是函数值)", other.type_name()
            ))),
        }
    }

    /// run_task_slice 执行当前任务的下一个调度切片（供调度器调用）。
    ///
    /// 前置条件：任务 VM 已 prepare_task_call（或从上次让出点继续）。
    /// 返回切片结果：
    ///   - Completed：任务执行完毕（正常返回或抛出异常）；
    ///   - Yielded：燃料耗尽让出，可再次调度；
    ///   - Parked：任务挂起（等待条件已由内置函数登记），唤醒前不要调度。
    pub(crate) fn run_task_slice(&mut self) -> crate::scheduler::SliceOutcome {
        // 任务体是内置函数：预执行结果直接作为任务结果
        if let Some(r) = self.prepared_result.take() {
            return crate::scheduler::SliceOutcome::Completed(r);
        }
        self.fuel = TASK_SLICE_FUEL;
        self.pause = Pause::None;
        let res = self.execute_frames();
        match res.kind {
            FlowKind::Yield => match self.pause {
                Pause::Parked | Pause::ParkedInject => crate::scheduler::SliceOutcome::Parked,
                _ => crate::scheduler::SliceOutcome::Yielded,
            },
            FlowKind::Throw => crate::scheduler::SliceOutcome::Completed(Err(res.value)),
            _ => crate::scheduler::SliceOutcome::Completed(Ok(res.value)),
        }
    }

    /// is_in_defer_context 判断当前是否处于 defer 执行上下文（栈顶帧在收尾状态）。
    ///
    /// 用于挂起类内置函数的前置检查：defer 中挂起需条目回队重试，
    /// 调用方（内置函数）应改走阻塞路径或报错。
    pub(crate) fn is_in_defer_context(&self) -> bool {
        self.frames.last().map_or(false, |f| f.finish.is_some())
    }

    /// stack_last_mut 取操作数栈顶的可变引用（调度器结果注入用；
    /// 挂起点的栈顶为 undefined 占位值）。
    pub(crate) fn stack_last_mut(&mut self) -> Option<&mut Value> {
        self.stack.last_mut()
    }

    /// set_pause 标记当前切片为"任务挂起"（scheduler 的 park 入口用）。
    pub(crate) fn set_pause(&mut self, kind: Pause) {
        self.pause = kind;
    }

    /// is_park_disabled 挂起是否被禁用（回调执行期间）。
    pub(crate) fn is_park_disabled(&self) -> bool {
        self.park_disabled > 0
    }

    /// set_pending_throw 写入待投递异常（调度器在恢复阻塞卸载任务时调用）。
    pub(crate) fn set_pending_throw(&mut self, e: Value) {
        self.pending_throw = Some(e);
    }

    /// push_park_disabled 递增挂起禁用计数（回调执行期间）。
    pub(crate) fn push_park_disabled(&mut self) {
        self.park_disabled += 1;
    }

    /// pop_park_disabled 递减挂起禁用计数。
    pub(crate) fn pop_park_disabled(&mut self) {
        self.park_disabled -= 1;
    }

    fn push(&mut self, v: Value) {
        self.stack.push(v);
    }
    fn pop(&mut self) -> Value {
        self.stack.pop().expect("stack underflow")
    }

    fn peek(&self) -> &Value {
        self.stack.last().expect("stack empty")
    }

    /// execute_frames 迭代式执行帧栈，直到顶层帧结束并返回其结果。
    ///
    /// 阶段一（解释器迭代化）：函数调用不再递归进入 run_frame，而是把调用方
    /// 帧压回 self.frames 后压入被调帧，由本循环统一驱动。任意深度的 Sflang
    /// 调用链只占用固定大小的 Rust 栈帧，任务状态（帧栈 + 操作数栈）可整体
    /// 保存/恢复——这是 goroutine 式调度（任务挂起/恢复）的前提。
    ///
    /// 每轮循环：
    ///   1. 取出栈顶帧；若处于收尾状态（finish），先按收尾状态机发起下一个
    ///      defer 调用（经帧栈迭代）或结束该帧；
    ///   2. 执行指令直到：控制流事件（ev）/ 发起用户函数调用（压帧换栈顶）；
    ///   3. 事件交 dispatch_event 在本帧 try 栈处置（进 catch / 挂起进
    ///      finally / 穿透出帧）——与原递归版语义一致；
    ///   4. 帧逻辑结束后有 defer 则进入收尾状态；否则按 resume 协议交付结果。
    fn execute_frames(&mut self) -> FlowResult {
        'machine: loop {
            // ---- 阻塞卸载的错误投递：恢复执行时在栈顶帧抛出（调用点可捕获） ----
            // 阻塞卸载的任务恢复正常执行时，栈顶帧 ip 在卸载调用之后；
            // 抛出走本帧 try 栈（catch/finally/向上传播），语义等同内联抛出。
            if let Some(e) = self.pending_throw.take() {
                match Self::dispatch_event_parts(&mut self.stack, self.frames.last_mut().expect("execute_frames: 帧栈为空"), Event::Throw(e)) {
                    DispatchOutcome::Continue => continue 'machine,
                    DispatchOutcome::Done(r) => {
                        if self.frames.last().map_or(false, |f| f.resume == Resume::PushResult) {
                            self.depth -= 1;
                        }
                        let frame = self.frames.pop().expect("execute_frames: 帧栈为空");
                        match self.conclude_frame(frame, r) {
                            Some(final_res) => return final_res,
                            None => continue 'machine,
                        }
                    }
                }
            }

            // ---- 收尾状态机：栈顶帧正在逆序执行 defers（非热点路径，整帧取出处理） ----
            if self.frames.last().map_or(false, |f| f.finish.is_some()) {
                let mut frame = self.frames.pop().unwrap();
                if let Some(fin) = frame.finish.as_mut() {
                if let Some(d) = fin.remaining_defers.pop() {
                    let argc = d.args.len();
                    // 本帧以收尾状态压回，defer 调用作为普通调用发起
                    self.frames.push(frame);
                    self.push(d.callee.clone());
                    for a in &d.args {
                        self.push(a.clone());
                    }
                    match self.start_call(argc) {
                        // 内置函数 defer 正常完成（返回值按语义丢弃）
                        Ok(_v) => continue 'machine,
                        Err(CallErr::Parked) => {
                            // defer 中等待-重试型原语挂起（如 defer lock(mu)）：
                            // 把 defer 条目放回队列，唤醒后重试整个 defer 调用。
                            // start_call 的 Parked 分支回推了实参，此处清掉
                            // （重试时重新压入）；值注入型在 defer 中被 park
                            // 入口拒绝（见 park_current_task_inject），不会到达。
                            let top = self.frames.last_mut().unwrap();
                            top.finish.as_mut().unwrap().remaining_defers.push(d);
                            let sl = self.stack.len();
                            self.stack.truncate(sl - (argc + 1));
                            return FlowResult { value: Value::Undefined, kind: FlowKind::Yield };
                        }
                        Err(CallErr::Thrown(e)) => {
                            // 内置 defer 抛错：记入栈顶收尾帧的 defer_err
                            // （后执行的覆盖先前的；继续执行剩余 defer，与原递归语义一致）
                            let top = self.frames.last_mut().unwrap();
                            top.finish.as_mut().unwrap().defer_err = Some(e);
                            continue 'machine;
                        }
                        Err(CallErr::EnterFrame(cf)) => {
                            // 用户函数 defer：被调帧入栈（收尾帧在其下），交回机器
                            self.frames.push(cf);
                            continue 'machine;
                        }
                    }
                }
                // 剩余 defer 为空：帧最终结束（深度已在进入收尾时扣减）
                let fin = frame.finish.take().unwrap();
                let result = match fin.defer_err {
                    Some(e) => FlowResult { value: e, kind: FlowKind::Throw },
                    None => fin.result,
                };
                match self.deliver_frame_result(frame, result) {
                    Some(final_res) => return final_res,
                    None => continue 'machine,
                }
                }
            } // 收尾状态机结束（正常路径不进入此块）

            // ---- 正常指令执行（字段级借用：帧就地于帧栈——免 Frame 移动、
            //      免 Arc 克隆、免队列往返；借用内只允许直接字段访问 self.stack /
            //      self.globals / self.extra_builtins / self.fuel 等，与 self.frames
            //      不相交）----
            let mut ev: Option<Event> = None;
            // 需要完整 &mut self 的操作，记入 pending_*，借用结束后执行：
            let mut pending_call: Option<(usize, usize)> = None; // (argc, 指令长度——Parked 回退用)
            let mut pending_import: Option<(String, String)> = None; // (path, cur_file)
            let mut pending_run: Option<(Value, Vec<Value>)> = None;
            let mut yield_out = false;
            {
                let frame = self.frames.last_mut().expect("execute_frames: 帧栈为空");
                let insts = &frame.code.insts;
                // 指令循环：任何错误/return/throw 都置 ev 后 break，交由事件处置
                while frame.ip < insts.len() {
                    // 切片燃料预算：每条指令扣 1，耗尽让出切片（帧已就地，直接返回）
                    if self.fuel == 0 {
                        self.pause = Pause::YieldFuel;
                        yield_out = true;
                        break;
                    }
                    self.fuel -= 1;
                let op_byte = insts[frame.ip];
                let op = match Opcode::from_u8(op_byte) {
                    Some(o) => o,
                    None => {
                        let ip = frame.ip;
                        ev = Some(Event::Throw(error_value(format!("invalid opcode: 0x{:02x} at ip={}", op_byte, ip))));
                        break;
                    }
                };
                match op {
                    Opcode::Null => { self.stack.push(Value::Undefined); frame.ip += 1; }
                    Opcode::Const => {
                        let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        self.stack.push(frame.code.constants[idx].clone());
                    }
                    Opcode::Pop => { self.stack.pop().expect("stack underflow"); frame.ip += 1; }
                    Opcode::Dup => { let v = self.stack.last().expect("stack empty").clone(); self.stack.push(v); frame.ip += 1; }
                    Opcode::LoadName => {
                        let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        let name = &frame.code.names[idx];
                        // 名字解析：globals → builtins → undefined（宽容策略，对齐 Charlang）。
                        // 读取未定义变量不再抛错，而是返回 undefined；AI/用户可用
                        // explainUndef(name) 主动诊断为何得到 undefined。
                        let resolved: Value = {
                            let globals = self.globals.lock().unwrap();
                            if let Some(v) = globals.get(name) {
                                v.clone()
                            } else if let Some(b) = Self::lookup_builtin_in(&self.extra_builtins, name) {
                                Value::Builtin(b)
                            } else {
                                // 未定义：返回 undefined（不抛错）
                                Value::Undefined
                            }
                        };
                        self.stack.push(resolved);
                    }
                    Opcode::StoreName => {
                        let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        let name = frame.code.names[idx].clone();
                        let v = self.stack.pop().expect("stack underflow");
                        self.globals.lock().unwrap().insert(name, v);
                    }
                    Opcode::AssignName => {
                        let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        let name = frame.code.names[idx].clone();
                        let v = self.stack.pop().expect("stack underflow");
                        // 简化：直接写全局（无论是否存在）
                        self.globals.lock().unwrap().insert(name, v);
                    }
                    Opcode::LoadGlobal => {
                        let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        let name = &frame.code.names[idx];
                        // 同 LoadName：未定义的全局返回 undefined（宽容策略）。
                        let resolved: Value = {
                            let globals = self.globals.lock().unwrap();
                            if let Some(v) = globals.get(name) {
                                v.clone()
                            } else if let Some(b) = Self::lookup_builtin_in(&self.extra_builtins, name) {
                                Value::Builtin(b)
                            } else {
                                Value::Undefined
                            }
                        };
                        self.stack.push(resolved);
                    }
                    Opcode::StoreGlobal => {
                        let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        let name = frame.code.names[idx].clone();
                        let v = self.stack.pop().expect("stack underflow");
                        self.globals.lock().unwrap().insert(name, v);
                    }
                    Opcode::LoadLocal => {
                        let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        if let Some(b) = frame.boxes.get(&idx) {
                            self.stack.push(b.lock().unwrap().clone());
                        } else {
                            self.stack.push(frame.locals[idx].clone());
                        }
                    }
                    Opcode::StoreLocal => {
                        let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        let v = self.stack.pop().expect("stack underflow");
                        if let Some(b) = frame.boxes.get(&idx) {
                            *b.lock().unwrap() = v;
                        } else {
                            frame.locals[idx] = v;
                        }
                    }
                    Opcode::LoadFree => {
                        let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        self.stack.push(frame.free_vars[idx].lock().unwrap().clone());
                    }
                    Opcode::StoreFree => {
                        let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        *frame.free_vars[idx].lock().unwrap() = self.stack.pop().expect("stack underflow");
                    }
                    Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Mod
                    | Opcode::BitAnd | Opcode::BitOr | Opcode::BitXor | Opcode::BitShl | Opcode::BitShr => {
                        let b = self.stack.pop().expect("stack underflow");
                        let a = self.stack.pop().expect("stack underflow");
                        match arith_op(op, a.clone(), b.clone()) {
                            Ok(r) => self.stack.push(r),
                            Err(e) => {
                                let line = frame.code.get_line(frame.ip);
                                let detail = format!("{} (行 {}: {} {:?} {} [{}] 和 {} [{}])",
                                    e, line, "运算", op, a.type_name(), a.inspect(), b.type_name(), b.inspect());
                                ev = Some(Event::Throw(error_value(detail)));
                                break;
                            }
                        }
                        frame.ip += 1;
                    }
                    Opcode::Neg => {
                        let a = self.stack.pop().expect("stack underflow");
                        match a {
                            // wrapping_neg：i64::MIN 取负仍为 MIN（溢出不 panic）
                            Value::Int(i) => self.stack.push(Value::Int(i.wrapping_neg())),
                            Value::Float(f) => self.stack.push(Value::Float(-f)),
                            _ => {
                                ev = Some(Event::Throw(error_value(format!(
                                    "cannot negate {} (可能原因：- 仅支持数值类型；bigInt 可用 bigInt(0) - x)", a.type_name(),
                                ))));
                                break;
                            }
                        }
                        frame.ip += 1;
                    }
                    Opcode::BitNot => {
                        // 按位取反 ~（整数或字节）
                        let a = self.stack.pop().expect("stack underflow");
                        match a {
                            Value::Int(i) => self.stack.push(Value::Int(!i)),
                            Value::Byte(b) => self.stack.push(Value::Byte(!b)),
                            _ => {
                                ev = Some(Event::Throw(error_value(format!(
                                    "cannot bitwise-not {} (可能原因：~ 仅支持整数/字节)", a.type_name(),
                                ))));
                                break;
                            }
                        }
                        frame.ip += 1;
                    }
                    Opcode::Eq => {
                        let b = self.stack.pop().expect("stack underflow");
                        let a = self.stack.pop().expect("stack underflow");
                        self.stack.push(Value::Bool(a.equals(&b)));
                        frame.ip += 1;
                    }
                    Opcode::Neq => {
                        let b = self.stack.pop().expect("stack underflow");
                        let a = self.stack.pop().expect("stack underflow");
                        self.stack.push(Value::Bool(!a.equals(&b)));
                        frame.ip += 1;
                    }
                    Opcode::LT | Opcode::LE | Opcode::GT | Opcode::GE => {
                        let b = self.stack.pop().expect("stack underflow");
                        let a = self.stack.pop().expect("stack underflow");
                        match cmp_op(op, a, b) {
                            Ok(r) => self.stack.push(r),
                            Err(e) => {
                                ev = Some(Event::Throw(error_value(e)));
                                break;
                            }
                        }
                        frame.ip += 1;
                    }
                    Opcode::Not => {
                        let a = self.stack.pop().expect("stack underflow");
                        self.stack.push(Value::Bool(!a.is_truthy()));
                        frame.ip += 1;
                    }
                    Opcode::Jump => {
                        let target = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip = target;
                    }
                    Opcode::JumpIfFalse => {
                        let cond = self.stack.pop().expect("stack underflow");
                        let target = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        if !cond.is_truthy() {
                            frame.ip = target;
                        }
                    }
                    Opcode::JumpIfTrue => {
                        let cond = self.stack.pop().expect("stack underflow");
                        let target = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        if cond.is_truthy() {
                            frame.ip = target;
                        }
                    }
                    Opcode::JumpIfNotUndefined => {
                        // 弹出栈顶，仅当该值不是 undefined 时跳转（用于 ?? 短路）
                        let v = self.stack.pop().expect("stack underflow");
                        let target = Code::read_u16(&insts, frame.ip + 1) as usize;
                        frame.ip += 3;
                        if !matches!(v, Value::Undefined) {
                            frame.ip = target;
                        }
                    }
                Opcode::CompoundIndex => {
                    // a[i] op= v：栈 [v, obj, idx] → [new]，地址只求值一次
                    let flag = insts[frame.ip + 1];
                    frame.ip += 2;
                    let idx = self.stack.pop().expect("stack underflow");
                    let obj = self.stack.pop().expect("stack underflow");
                    let v = self.stack.pop().expect("stack underflow");
                    match Self::compound_index(&obj, &idx, v, flag) {
                        Ok(r) => self.stack.push(r),
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    }
                }
                Opcode::CompoundMember => {
                    // obj.k op= v：栈 [v, obj] → [new]
                    let name_idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    let flag = insts[frame.ip + 3];
                    frame.ip += 4;
                    let name = frame.code.names[name_idx].clone();
                    let obj = self.stack.pop().expect("stack underflow");
                    let v = self.stack.pop().expect("stack underflow");
                    match Self::compound_member(&obj, &name, v, flag) {
                        Ok(r) => self.stack.push(r),
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    }
                }
                Opcode::IncDecIndex => {
                    // a[i]++ / ++a[i]：栈 [obj, idx] → [result]
                    // 前缀返回新值，后缀返回旧值（Float/BigInt 也正确）
                    let flag = insts[frame.ip + 1];
                    frame.ip += 2;
                    let idx = self.stack.pop().expect("stack underflow");
                    let obj = self.stack.pop().expect("stack underflow");
                    let inc = flag & 0x01 == 0; // 0=Inc, 1=Dec
                    match Self::incdec_index(&obj, &idx, inc) {
                        Ok((old, new)) => {
                            let result = if flag & 0x80 != 0 { old } else { new };
                            self.stack.push(result);
                        }
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    }
                }
                Opcode::IncDecMember => {
                    // obj.k++ / ++obj.k：栈 [obj] → [result]
                    let name_idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    let flag = insts[frame.ip + 3];
                    frame.ip += 4;
                    let name = frame.code.names[name_idx].clone();
                    let obj = self.stack.pop().expect("stack underflow");
                    let inc = flag & 0x01 == 0;
                    match Self::incdec_member(&obj, &name, inc) {
                        Ok((old, new)) => {
                            let result = if flag & 0x80 != 0 { old } else { new };
                            self.stack.push(result);
                        }
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    }
                }
                Opcode::Slice => {
                    // 切片 a[low:high]：栈 [obj, low, high] → [result]
                    // low/high 缺省为 undefined（表示到边界）
                    frame.ip += 1;
                    let high = self.stack.pop().expect("stack underflow");
                    let low = self.stack.pop().expect("stack underflow");
                    let obj = self.stack.pop().expect("stack underflow");
                    let lo: Option<i64> = match low {
                        Value::Undefined => None,
                        Value::Int(i) => Some(i),
                        v => {
                            ev = Some(Event::Throw(error_value(format!(
                                "切片下界需为 int 或缺省，得到 {} (可能原因：语法错误)", v.type_name(),
                            ))));
                            break;
                        }
                    };
                    let hi: Option<i64> = match high {
                        Value::Undefined => None,
                        Value::Int(i) => Some(i),
                        v => {
                            ev = Some(Event::Throw(error_value(format!(
                                "切片上界需为 int 或缺省，得到 {} (可能原因：语法错误)", v.type_name(),
                            ))));
                            break;
                        }
                    };
                    match slice_value(&obj, lo, hi) {
                        Ok(v) => self.stack.push(v),
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    }
                }
                Opcode::MethodCall => {
                    // 方法调用 obj.name(args)，自动注入 obj 作为隐式 self（首参）
                    // 操作数：u16 name_idx, u8 argc。栈：[obj, arg1, ..., argN]
                    let name_idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    let argc = insts[frame.ip + 3] as usize;
                    frame.ip += 4;
                    let name = frame.code.names[name_idx].clone();
                    // 弹出 N 个参数 + obj（参数在上，obj 在底）
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.stack.pop().expect("stack underflow"));
                    }
                    args.reverse(); // 恢复 arg1..argN 顺序
                    let obj = self.stack.pop().expect("stack underflow");
                    // 从 obj 读取方法（沿原型链）
                    let method = match member_get(&obj, &name) {
                        Ok(v) => v,
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    };
                    // 重排栈为 do_call 期望的 [callee=method, self=obj, arg1, ..., argN]
                    self.stack.push(method);
                    self.stack.push(obj); // 隐式 self
                    for a in args {
                        self.stack.push(a);
                    }
                    // 调用：argc = N + 1（含隐式 self）；借用外发起
                    pending_call = Some((argc + 1, 4));
                    break;
                }
                Opcode::SpreadCall => {
                    // 带展开的调用：u8 argc, u64 spread_mask
                    // 栈：[callee, arg0, arg1, ...]（标记为 spread 的 arg 是 array）
                    let argc = insts[frame.ip + 1] as usize;
                    let spread_mask = Code::read_u64(&insts, frame.ip + 2);
                    frame.ip += 10;
                    let all_args = match Self::expand_spread_args(&mut self.stack, argc, spread_mask) {
                        Ok(a) => a,
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    };
                    let callee = self.stack.pop().expect("stack underflow");
                    self.stack.push(callee);
                    for a in &all_args {
                        self.stack.push(a.clone());
                    }
                    // 借用外发起（展开后的实参已在栈上）
                    pending_call = Some((all_args.len(), 10));
                    break;
                }
                Opcode::MethodSpreadCall => {
                    // 带展开的方法调用：u16 name_idx, u8 argc, u64 spread_mask
                    // 栈：[obj, arg0, ...]（标记为 spread 的 arg 是 array）
                    let name_idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    let argc = insts[frame.ip + 3] as usize;
                    let spread_mask = Code::read_u64(&insts, frame.ip + 4);
                    frame.ip += 12;
                    let name = frame.code.names[name_idx].clone();
                    let all_args = match Self::expand_spread_args(&mut self.stack, argc, spread_mask) {
                        Ok(a) => a,
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    };
                    let obj = self.stack.pop().expect("stack underflow");
                    let method = match member_get(&obj, &name) {
                        Ok(v) => v,
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    };
                    // 重排为 [callee=method, self=obj, 展开后的 args]
                    self.stack.push(method);
                    self.stack.push(obj);
                    for a in &all_args {
                        self.stack.push(a.clone());
                    }
                    // 借用外发起（argc 含隐式 self；展开后的实参已在栈上）
                    pending_call = Some((all_args.len() + 1, 12));
                    break;
                }
                Opcode::Call => {
                    let argc = insts[frame.ip + 1] as usize;
                    frame.ip += 2;
                    // start_call 需要完整 &mut self（执行内置函数），借用外发起
                    pending_call = Some((argc, 2));
                    break;
                }
                Opcode::Return => {
                    let v = self.stack.pop().expect("stack underflow");
                    ev = Some(Event::Return(v));
                    break;
                }
                Opcode::ReturnVoid => {
                    ev = Some(Event::Return(Value::Undefined));
                    break;
                }
                Opcode::Closure => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let tmpl = match &frame.code.constants[idx] {
                        Value::Func(f) => f.clone(),
                        _ => {
                            ev = Some(Event::Throw(error_value("closure: constant is not a function")));
                            break;
                        }
                    };
                    // 提取 free_vars
                    let mut free_vars = Vec::with_capacity(tmpl.body.free_sources.len());
                    for src in &tmpl.body.free_sources {
                        if src.is_local {
                            if !frame.boxes.contains_key(&src.index) {
                                let b = Arc::new(Mutex::new(frame.locals[src.index].clone()));
                                frame.boxes.insert(src.index, b);
                            }
                            free_vars.push(frame.boxes.get(&src.index).unwrap().clone());
                        } else {
                            free_vars.push(frame.free_vars[src.index].clone());
                        }
                    }
                    let func = Function::new_closure(
                        tmpl.name.clone(),
                        tmpl.params.clone(),
                        tmpl.body.clone(),
                        free_vars,
                        tmpl.variadic,
                    );
                    self.stack.push(Value::Func(Arc::new(func)));
                }
                Opcode::BuildArray => {
                    let n = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let stack_len = self.stack.len();
                    let elems: Vec<Value> = self.stack[stack_len - n..].to_vec();
                    self.stack.truncate(stack_len - n);
                    self.stack.push(Value::Array(Arc::new(Mutex::new(elems))));
                }
                Opcode::BuildMap => {
                    let n = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let mut map = crate::object_map::Map::new();
                    for _ in 0..n {
                        let v = self.stack.pop().expect("stack underflow");
                        let k = self.stack.pop().expect("stack underflow");
                        match k {
                            Value::Str(s) => map.set((*s).to_string(), v),
                            _ => {
                                ev = Some(Event::Throw(error_value(format!("map key must be string, got {}", k.type_name()))));
                                break;
                            }
                        }
                    }
                    if ev.is_some() { break; }
                    self.stack.push(Value::Object(Arc::new(Mutex::new(map))));
                }
                Opcode::BuildOrdMap => {
                    let n = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    // 栈顶为最后一对，弹出后逆序存放，再反转保持插入顺序
                    let mut temp: Vec<(String, Value)> = Vec::with_capacity(n);
                    for _ in 0..n {
                        let v = self.stack.pop().expect("stack underflow");
                        let k = self.stack.pop().expect("stack underflow");
                        match k {
                            Value::Str(s) => temp.push(((*s).to_string(), v)),
                            _ => {
                                ev = Some(Event::Throw(error_value(format!("map key must be string, got {}", k.type_name()))));
                                break;
                            }
                        }
                    }
                    if ev.is_some() { break; }
                    temp.reverse();  // 恢复插入顺序
                    let mut map = crate::ord_map::OrdMap::new();
                    for (k, v) in temp {
                        map.set(k, v);
                    }
                    self.stack.push(Value::Map(Arc::new(Mutex::new(map))));
                }
                Opcode::IndexGet => {
                    frame.ip += 1;
                    let idx = self.stack.pop().expect("stack underflow");
                    let obj = self.stack.pop().expect("stack underflow");
                    match index_get(&obj, &idx) {
                        Ok(v) => self.stack.push(v),
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    }
                }
                Opcode::IndexSet => {
                    frame.ip += 1;
                    // 栈形如：[..., v, a, i]（由 compiler 的 Assign Index 路径产生）
                    // IndexSet 语义：弹 i, a, v（v 在底），执行 a[i] = v，不压回
                    // （赋值表达式的结果值 v 已由编译器预先留在栈底）
                    let i = self.stack.pop().expect("stack underflow");
                    let a = self.stack.pop().expect("stack underflow");
                    let v = self.stack.pop().expect("stack underflow");
                    match index_set(&a, &i, v) {
                        Ok(_) => {}
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    }
                }
                Opcode::GetMember => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let name = frame.code.names[idx].clone();
                    let obj = self.stack.pop().expect("stack underflow");
                    match member_get(&obj, &name) {
                        Ok(v) => self.stack.push(v),
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    }
                }
                Opcode::SetMember => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let name = frame.code.names[idx].clone();
                    // 栈：[..., v, a]（v 在下，a 在上）
                    let a = self.stack.pop().expect("stack underflow");
                    let v = self.stack.pop().expect("stack underflow");
                    match member_set(&a, &name, v) {
                        Ok(_) => {}
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    }
                }
                Opcode::PushTry => {
                    // 压入 try 入口：三阶段状态机（Body/Catch/Finally），
                    // 入口常驻 try 栈直到整条 try 语句结束（ExitFinally/异常穿透）。
                    // snapshot 记录当前操作数栈深度，进入 catch/finally 前回退，
                    // 清理半求值表达式残留的操作数。
                    let catch_ip = Code::read_u16(&insts, frame.ip + 1);
                    let finally_ip = Code::read_u16(&insts, frame.ip + 3);
                    let end_ip = Code::read_u16(&insts, frame.ip + 5);
                    frame.ip += 7;
                    frame.try_stack.push(TryEntry {
                        catch_ip: if catch_ip == u16::MAX { NO_IP } else { catch_ip as usize },
                        finally_ip: if finally_ip == u16::MAX { NO_IP } else { finally_ip as usize },
                        end_ip: end_ip as usize,
                        phase: TryPhase::Body,
                        snapshot: self.stack.len(),
                        pending: None,
                    });
                }
                Opcode::TryBodyEnd => {
                    // try 块正常结束：有 finally 则进入（挂起为空），否则弹出入口顺序执行
                    let enter = match frame.try_stack.last() {
                        Some(te) if te.finally_ip != NO_IP => Some((te.finally_ip, te.snapshot)),
                        _ => None,
                    };
                    match enter {
                        Some((fip, snap)) => {
                            let te = frame.try_stack.last_mut().unwrap();
                            te.phase = TryPhase::Finally;
                            te.pending = None;
                            if self.stack.len() > snap { self.stack.truncate(snap); }
                            frame.ip = fip;
                        }
                        None => {
                            frame.try_stack.pop();
                            frame.ip += 1;
                        }
                    }
                }
                Opcode::CatchEnd => {
                    // catch 块正常结束：有 finally 则进入（挂起为空），否则弹出入口顺序执行
                    let enter = match frame.try_stack.last() {
                        Some(te) if te.finally_ip != NO_IP => Some((te.finally_ip, te.snapshot)),
                        _ => None,
                    };
                    match enter {
                        Some((fip, snap)) => {
                            let te = frame.try_stack.last_mut().unwrap();
                            te.phase = TryPhase::Finally;
                            te.pending = None;
                            if self.stack.len() > snap { self.stack.truncate(snap); }
                            frame.ip = fip;
                        }
                        None => {
                            frame.try_stack.pop();
                            frame.ip += 1;
                        }
                    }
                }
                Opcode::ExitFinally => {
                    // finally 块结束：弹出入口，恢复挂起的控制流或继续到语句末尾。
                    // finally 内若有 return/throw，事件在 dispatch 中已把本入口弹出
                    // 并向外传播（挂起值被覆盖丢弃），不会走到这里。
                    let popped = frame.try_stack.pop();
                    let (pending, end_ip) = match popped {
                        Some(te) => (te.pending, te.end_ip),
                        None => (None, frame.ip + 1),
                    };
                    match pending {
                        Some(PendingFlow::Return(v)) => {
                            ev = Some(Event::Return(v));
                            break;
                        }
                        Some(PendingFlow::Throw(v)) => {
                            ev = Some(Event::Throw(v));
                            break;
                        }
                        Some(PendingFlow::Jump { target, leave }) => {
                            ev = Some(Event::Jump { target, leave });
                            break;
                        }
                        None => {
                            frame.ip = end_ip;
                        }
                    }
                }
                Opcode::LeaveLoop => {
                    // break/continue 穿越 try 语句：目标 + 待穿越层数。
                    // 由 dispatch_event 逐层离开 try 入口（有 finally 的先进 finally
                    // 并挂起本跳转），全部离开后跳到目标。
                    let target = Code::read_u16(&insts, frame.ip + 1) as usize;
                    let leave = insts[frame.ip + 3];
                    frame.ip += 4;
                    ev = Some(Event::Jump { target, leave });
                    break;
                }
                Opcode::Throw => {
                    frame.ip += 1;
                    let v = self.stack.pop().expect("stack underflow");
                    ev = Some(Event::Throw(v));
                    break;
                }
                Opcode::Defer => {
                    let argc = insts[frame.ip + 1] as usize;
                    frame.ip += 2;
                    // 栈：[callee, arg0, arg1, ...]
                    let stack_len = self.stack.len();
                    let callee = self.stack[stack_len - argc - 1].clone();
                    let args: Vec<Value> = self.stack[stack_len - argc..].to_vec();
                    self.stack.truncate(stack_len - argc - 1);
                    frame.defers.push(DeferEntry { callee, args });
                }
                Opcode::Run => {
                    let argc = insts[frame.ip + 1] as usize;
                    frame.ip += 2;
                    // 启动任务执行调用（spawn_thread 需要完整 &mut self，借用外执行）
                    let stack_len = self.stack.len();
                    let callee = self.stack[stack_len - argc - 1].clone();
                    let args: Vec<Value> = self.stack[stack_len - argc..].to_vec();
                    self.stack.truncate(stack_len - argc - 1);
                    pending_run = Some((callee, args));
                    break;
                }
                Opcode::Import => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let path = frame.code.names[idx].clone();
                    let cur_file = frame.code.file.clone();
                    // import 是语句，成功不产生值（保持操作数栈平衡）；
                    // do_import 需要完整 &mut self，借用外执行
                    pending_import = Some((path, cur_file));
                    break;
                }
                Opcode::Ref => {
                    // &expr：创建引用包装
                    // 对基本类型（Int/Float/Bool/String/Byte）：创建 Mutex<Value> 拷贝
                    // 对引用类型（Array/Object/Map）：已经是 Arc<Mutex>，直接包装 Value
                    // 无论哪种，*p = v 都能修改引用内的值
                    frame.ip += 1;
                    let v = self.stack.pop().expect("stack underflow");
                    self.stack.push(Value::Native(std::sync::Arc::new(std::sync::Arc::new(std::sync::Mutex::new(v)))));
                }
                Opcode::Deref => {
                    // *expr：弹出引用包装，读取内部值
                    frame.ip += 1;
                    let v = self.stack.pop().expect("stack underflow");
                    match deref_value(&v) {
                        Ok(inner) => self.stack.push(inner),
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    }
                }
                Opcode::SetDeref => {
                    // *p = v：栈 [v, ref]，弹 ref 和 v，写入
                    frame.ip += 1;
                    let ref_val = self.stack.pop().expect("stack underflow");
                    let new_val = self.stack.pop().expect("stack underflow");
                    match set_deref_value(&ref_val, new_val) {
                        Ok(()) => {
                            // 编译器在 SetDeref 前留了一份 v 在栈底作为赋值表达式的结果
                        }
                        Err(e) => { ev = Some(Event::Throw(error_value(e))); break; }
                    }
                }
                }
            } // while 指令循环
            } // 借用作用域结束（帧就地留在帧栈）

            // 燃料耗尽：让出切片（帧已就地，下个切片从同一 ip 继续）
            if yield_out {
                return FlowResult { value: Value::Undefined, kind: FlowKind::Yield };
            }

            // Run/Import：借用结束后执行（需要完整 &mut self）；均为语句，
            // 完成后回到指令循环继续
            if let Some((callee, args)) = pending_run.take() {
                self.spawn_thread(callee, args);
                continue 'machine;
            }
            if let Some((path, cur_file)) = pending_import.take() {
                match self.do_import(&path, &cur_file) {
                    Ok(()) => continue 'machine,
                    Err(err_val) => { ev = Some(Event::Throw(err_val)); }
                }
            }

            // 函数调用：借用外发起（start_call 执行内置函数需要完整 &mut self）
            if let Some((argc, ilen)) = pending_call.take() {
                match self.start_call(argc) {
                    Ok(v) => {
                        self.push(v);
                        if self.pause == Pause::ParkedInject {
                            // 值注入型挂起（chanRecv/onceDo 等待者/sleep）：结果位已压
                            // 占位值（唤醒后由调度器注入真实结果），帧 ip 已越过本指令，
                            // 帧已就地，立即让出切片
                            return FlowResult { value: Value::Undefined, kind: FlowKind::Yield };
                        }
                        // 内置函数调用完成：回到指令循环继续（调用发生在帧中部）
                        continue 'machine;
                    }
                    Err(CallErr::Thrown(e)) => { ev = Some(Event::Throw(e)); }
                    Err(CallErr::Parked) => {
                        // 等待-重试型挂起（lock/wgWait/semAcquire 等）：实参已回推，
                        // 回退 ip 到本调用指令，唤醒后重新执行本调用（重查条件）
                        self.frames.last_mut().unwrap().ip -= ilen;
                        return FlowResult { value: Value::Undefined, kind: FlowKind::Yield };
                    }
                    Err(CallErr::EnterFrame(cf)) => {
                        // 调用方帧已就地（ip 已推进），被调帧入栈，交回机器循环
                        self.frames.push(cf);
                        continue 'machine;
                    }
                }
            }

            // 指令循环结束：若无事件则自然结束（无 return）→ 返回 undefined
            let e = match ev {
                Some(e) => e,
                None => Event::Return(Value::Undefined),
            };
            // 事件在本帧 try 栈上处置（catch/finally/穿透/帧逻辑结束）；
            // stack 与 frames 是不相交字段，可同时可变借用
            match Self::dispatch_event_parts(&mut self.stack, self.frames.last_mut().expect("execute_frames: 帧栈为空"), e) {
                DispatchOutcome::Continue => {
                    // 已设置新 ip：帧就地，下一轮继续执行
                    continue 'machine;
                }
                DispatchOutcome::Done(r) => {
                    // 帧逻辑结束（try 栈走完）：扣减调用深度，弹出后收尾/交付
                    if self.frames.last().map_or(false, |f| f.resume == Resume::PushResult) {
                        self.depth -= 1;
                    }
                    let frame = self.frames.pop().expect("execute_frames: 帧栈为空");
                    match self.conclude_frame(frame, r) {
                        Some(final_res) => return final_res,
                        None => continue 'machine,
                    }
                }
            }
        } // 'machine
    }

    /// conclude_frame 处理"帧逻辑结束"（dispatch 返回 Done）：
    /// 有 defer 则转入收尾状态机（经帧栈迭代执行），无则直接交付结果。
    /// 返回 Some 表示整个 execute_frames 结束（结果交 Rust 调用方）。
    fn conclude_frame(&mut self, frame: Frame, result: FlowResult) -> Option<FlowResult> {
        if !frame.defers.is_empty() {
            let mut frame = frame;
            frame.finish = Some(FinishState {
                result,
                remaining_defers: std::mem::take(&mut frame.defers),
                defer_err: None,
            });
            self.frames.push(frame);
            return None;
        }
        self.deliver_frame_result(frame, result)
    }

    /// deliver_frame_result 把一帧的最终结果按 resume 协议交付。
    ///
    /// frame 已弹出帧栈。返回 Some 表示整个 execute_frames 结束。
    fn deliver_frame_result(&mut self, frame: Frame, result: FlowResult) -> Option<FlowResult> {
        match frame.resume {
            Resume::TopLevel => Some(result),
            Resume::PushResult => {
                let mut caller = self.frames.pop().expect("deliver_frame_result: 调用方帧缺失");
                // 调用方处于收尾状态：本结果是它的某个 defer 调用的结果
                if caller.finish.is_some() {
                    if result.kind == FlowKind::Throw {
                        // defer 调用抛错：记录（后执行的覆盖先前的）
                        caller.finish.as_mut().unwrap().defer_err = Some(result.value);
                    }
                    // Ok：defer 返回值无用途，丢弃
                    self.frames.push(caller);
                    return None;
                }
                if result.kind == FlowKind::Throw {
                    // 异常向调用方传播（可能被 catch/finally 接住，或调用方也结束）
                    match Self::dispatch_event_parts(&mut self.stack, &mut caller, Event::Throw(result.value)) {
                        DispatchOutcome::Continue => {
                            self.frames.push(caller);
                            None
                        }
                        DispatchOutcome::Done(r2) => {
                            // 调用方帧逻辑结束：扣深度，进入收尾或继续上传
                            if caller.resume == Resume::PushResult {
                                self.depth -= 1;
                            }
                            self.conclude_frame(caller, r2)
                        }
                    }
                } else {
                    // 正常返回：值压回操作数栈，调用方继续执行（ip 已在调用指令之后）
                    self.push(result.value);
                    self.frames.push(caller);
                    None
                }
            }
        }
    }

    /// start_call 发起一次调用（从操作数栈弹出 callee 与实参）。
    ///
    /// 栈布局：[callee, arg1, ..., argN]，调用后弹出 callee 与全部实参。
    /// 返回：
    ///   - Ok(v)：被调方是内置函数且已执行完成，v 为结果（调用方决定压栈或丢弃）；
    ///   - Err(Thrown(e))：调用失败（不可调用/超最大深度/内置抛错），e 为异常值；
    ///   - Err(EnterFrame(f))：被调方是用户函数——调用方须先压回自身帧
    ///     （ip 已推进过调用指令），再压入 f，交回机器循环（迭代式调用，不递归）。
    fn start_call(&mut self, argc: usize) -> Result<Value, CallErr> {
        let stack_len = self.stack.len();
        let callee = self.stack[stack_len - argc - 1].clone();
        let args: Vec<Value> = self.stack[stack_len - argc..].to_vec();
        self.stack.truncate(stack_len - argc - 1);

        match &callee {
            Value::Builtin(b) => {
                // 阻塞型内置函数（网络/文件 IO）+ 任务上下文：卸载到阻塞线程池。
                // 任务挂起（值注入型）等待结果，不占调度 worker——任务内慢 IO
                // 不再挤占并发容量。defer/回调中 park 被禁 → 原地执行（旧行为）。
                // 结果经注入送达；错误经 pending_throw 在恢复时于调用点抛出。
                if b.blocking && scheduler::can_offload_blocking(self) {
                    if scheduler::park_current_task_inject(self).is_ok() {
                        let task = scheduler::current_task().expect("offload: task");
                        let globals = self.globals.clone();
                        let out = self.out.clone();
                        let func = b.func;
                        let off_args: Vec<Value> = args.to_vec();
                        scheduler::submit_blocking_job(Box::new(move || {
                            // 阻塞池工作 VM：内置函数只用共享句柄（globals/out），
                            // 不触碰任务解释状态（标记前提，见 Builtin.blocking）
                            let mut bvm = VM::new();
                            bvm.set_globals_handle(globals);
                            bvm.set_output_handle(out);
                            let res = func(&mut bvm, &off_args);
                            scheduler::wake_task_with_result(&task, res);
                        }));
                        return Ok(Value::Undefined); // 占位值：调用点检测挂起后让出
                    }
                    // park 失败（上下文限制）→ 落回原地执行
                }
                let r = (b.func)(self, &args);
                if self.pause == Pause::Parked {
                    // 等待-重试型原语挂起（lock/rlock/wlock/wgWait/semAcquire）：
                    // 回推 callee 与实参（恢复指令开始时的栈形态），调用点回退 ip，
                    // 唤醒后重新执行本调用（重查条件）
                    self.push(callee.clone());
                    for a in &args {
                        self.push(a.clone());
                    }
                    return Err(CallErr::Parked);
                }
                r.map_err(CallErr::Thrown)
            }
            Value::Func(f) => {
                if self.depth >= self.max_call_depth {
                    return Err(CallErr::Thrown(error_value(format!(
                        "max call depth exceeded ({}); 可能原因：递归过深", self.max_call_depth
                    ))));
                }
                self.depth += 1;
                let mut new_frame = Frame::new(f.body.clone(), f.free_vars.clone());
                self.bind_params(f, &args, &mut new_frame);
                new_frame.resume = Resume::PushResult;
                Err(CallErr::EnterFrame(new_frame))
            }
            Value::Undefined => Err(CallErr::Thrown(error_value(
                "not callable: undefined (可能原因：调用了未定义的函数名；请检查函数是否已定义、拼写是否正确；内置函数可用 help(分类) 查询，未定义变量可用 explainUndef(\"名字\") 诊断)",
            ))),
            _ => Err(CallErr::Thrown(error_value(format!(
                "not callable: {} (可能原因：调用了非函数值；请检查变量是否为函数)", callee.type_name()
            )))),
        }
    }

    /// bind_params 绑定形参与实参。
    fn bind_params(&self, fn_def: &Function, args: &[Value], frame: &mut Frame) {
        let n = fn_def.params.len();
        if fn_def.variadic {
            for i in 0..n.saturating_sub(1) {
                frame.locals[i] = args.get(i).cloned().unwrap_or(Value::Undefined);
            }
            if n > 0 {
                let rest: Vec<Value> = if args.len() >= n - 1 {
                    args[n - 1..].to_vec()
                } else {
                    Vec::new()
                };
                frame.locals[n - 1] = Value::Array(Arc::new(Mutex::new(rest)));
            }
        } else {
            for i in 0..n {
                frame.locals[i] = args.get(i).cloned().unwrap_or(Value::Undefined);
            }
        }
    }

    /// dispatch_event_parts 处置帧内控制流事件（return/throw/跳转穿越）。
    ///
    /// 参数化 stack：调用方以不相交字段借用传入（&mut self.stack + 帧借用），
    /// 支持指令循环的就地帧执行模型。
    ///
    /// 事件沿 try 栈从内向外传播：
    ///   - Throw：body 阶段有 catch 则进 catch（异常值压栈供 catch 变量绑定）；
    ///     否则有 finally 则挂起进 finally；catch 阶段的异常只进 finally；
    ///     finally 阶段的异常直接弹出本入口向外传播（覆盖挂起值）。
    ///   - Return：body/catch 阶段有 finally 则挂起进 finally；finally 阶段直接
    ///     弹出入口向外（finally 的 return 覆盖先前挂起值）。
    ///   - Jump（break/continue 穿越）：逐层弹出穿越的入口（有 finally 的先挂起
    ///     进 finally，剩余层数记录在挂起值中），全部离开后跳到目标。
    /// try 栈为空时：执行本帧全部 defers（逆序，defer 错误覆盖帧结果）后结束帧。
    fn dispatch_event_parts(stack: &mut Vec<Value>, frame: &mut Frame, ev: Event) -> DispatchOutcome {
        match ev {
            Event::Throw(val) => {
                // 先增强错误信息（追加行号），再沿 try 栈传播
                let mut val = Self::enhance_error_with_line(frame, val);
                loop {
                    // 判定当前最内入口对 Throw 的处置方式
                    enum TAct { EnterCatch, EnterFinally, PopOutward, NoneEntry }
                    let act = match frame.try_stack.last() {
                        Some(te) => match te.phase {
                            TryPhase::Body if te.catch_ip != NO_IP => TAct::EnterCatch,
                            TryPhase::Body | TryPhase::Catch if te.finally_ip != NO_IP => TAct::EnterFinally,
                            _ => TAct::PopOutward,
                        },
                        None => TAct::NoneEntry,
                    };
                    match act {
                        TAct::EnterCatch => {
                            let (cip, snap) = {
                                let te = frame.try_stack.last_mut().unwrap();
                                te.phase = TryPhase::Catch;
                                (te.catch_ip, te.snapshot)
                            };
                            if stack.len() > snap { stack.truncate(snap); }
                            stack.push(val);
                            frame.ip = cip;
                            return DispatchOutcome::Continue;
                        }
                        TAct::EnterFinally => {
                            let (fip, snap) = {
                                let te = frame.try_stack.last_mut().unwrap();
                                te.phase = TryPhase::Finally;
                                te.pending = Some(PendingFlow::Throw(val));
                                (te.finally_ip, te.snapshot)
                            };
                            if stack.len() > snap { stack.truncate(snap); }
                            frame.ip = fip;
                            return DispatchOutcome::Continue;
                        }
                        TAct::PopOutward => {
                            frame.try_stack.pop();
                            // 继续向外传播同一 Throw
                        }
                        TAct::NoneEntry => {
                            return Self::finish_frame_with_defers(
                                frame,
                                FlowResult { value: val, kind: FlowKind::Throw },
                            );
                        }
                    }
                }
            }
            Event::Return(val) => {
                loop {
                    enum RAct { EnterFinally, PopOutward, NoneEntry }
                    let act = match frame.try_stack.last() {
                        Some(te) if te.phase != TryPhase::Finally && te.finally_ip != NO_IP => RAct::EnterFinally,
                        Some(_) => RAct::PopOutward,
                        None => RAct::NoneEntry,
                    };
                    match act {
                        RAct::EnterFinally => {
                            let (fip, snap) = {
                                let te = frame.try_stack.last_mut().unwrap();
                                te.phase = TryPhase::Finally;
                                te.pending = Some(PendingFlow::Return(val.clone()));
                                (te.finally_ip, te.snapshot)
                            };
                            if stack.len() > snap { stack.truncate(snap); }
                            frame.ip = fip;
                            return DispatchOutcome::Continue;
                        }
                        RAct::PopOutward => {
                            frame.try_stack.pop();
                        }
                        RAct::NoneEntry => {
                            return Self::finish_frame_with_defers(
                                frame,
                                FlowResult { value: val, kind: FlowKind::Return },
                            );
                        }
                    }
                }
            }
            Event::Jump { target, leave } => {
                let mut leave = leave;
                loop {
                    if leave == 0 {
                        frame.ip = target;
                        return DispatchOutcome::Continue;
                    }
                    enum JAct { EnterFinally, PopOutward, NoneEntry }
                    let act = match frame.try_stack.last() {
                        Some(te) if te.phase != TryPhase::Finally && te.finally_ip != NO_IP => JAct::EnterFinally,
                        Some(_) => JAct::PopOutward,
                        None => JAct::NoneEntry,
                    };
                    match act {
                        JAct::EnterFinally => {
                            let (fip, snap) = {
                                let te = frame.try_stack.last_mut().unwrap();
                                te.phase = TryPhase::Finally;
                                te.pending = Some(PendingFlow::Jump { target, leave: leave - 1 });
                                (te.finally_ip, te.snapshot)
                            };
                            if stack.len() > snap { stack.truncate(snap); }
                            frame.ip = fip;
                            return DispatchOutcome::Continue;
                        }
                        JAct::PopOutward => {
                            frame.try_stack.pop();
                            leave -= 1;
                        }
                        JAct::NoneEntry => {
                            // 编译器保证 leave 不超过实际 try 层数；防御性直接跳转
                            frame.ip = target;
                            return DispatchOutcome::Continue;
                        }
                    }
                }
            }
        }
    }

    /// finish_frame_with_defers 帧逻辑结束收尾（try 栈走完时调用）。
    ///
    /// 迭代化后本函数不再内联执行 defers——defers 留在帧上，由 execute_frames
    /// 的收尾状态机经帧栈逐个发起调用（见 FinishState）。此处仅清理 try 栈。
    /// defer 的执行语义（任何退出路径都执行、错误不中断剩余 defer、最后一个
    /// defer 错误覆盖帧原始结果）由收尾状态机保持，与原递归版一致。
    fn finish_frame_with_defers(frame: &mut Frame, result: FlowResult) -> DispatchOutcome {
        // 清理本帧残留的 try 入口（defers 执行期间不再有 try 语义）
        frame.try_stack.clear();
        DispatchOutcome::Done(result)
    }

    /// enhance_error_with_line 为未捕获的错误值追加行号信息。
    ///
    /// 只处理 Error 类型，跳过用户主动 throw 的非 Error 值。
    /// 如果错误消息已包含 "行 "（行号标记），不重复追加。
    fn enhance_error_with_line(frame: &Frame, val: Value) -> Value {
        match &val {
            Value::Error(e) => {
                if e.message.contains(" (行 ") {
                    return val;
                }
                // 当前 ip 的行号可能是 0（未设置 set_line），往前找最近的非零行号
                let mut line = 0u32;
                let ip = frame.ip;
                if ip > 0 && ip <= frame.code.lines.len() {
                    // 向前搜索最近的有行号的指令
                    for i in (0..ip.min(frame.code.lines.len())).rev() {
                        if frame.code.lines[i] > 0 {
                            line = frame.code.lines[i];
                            break;
                        }
                    }
                }
                if line > 0 {
                    Value::Error(Arc::new(SfError::new(format!(
                        "{} (行 {})", e.message, line
                    ))))
                } else {
                    val
                }
            }
            _ => val,
        }
    }

    /// finish_return 已由 dispatch_event + finish_frame_with_defers 取代（删除）。
    /// 旧行为（return 时挂起进 finally / 执行 defers）现由事件状态机统一处理。

    /// do_import 加载并执行一个 Sflang 脚本，将其顶层 var/func 合并到当前全局环境。
    ///
    /// 实现要点：
    ///   - 路径解析：相对路径基于当前脚本（cur_file）所在目录；绝对路径直接使用
    ///   - 环检测：用规范化绝对路径的栈防止循环 import（A import B import A）
    ///   - 全局合并：目标脚本的顶层声明写入同一 self.globals，调用方即可引用
    ///   - 模块缓存：已加载的模块不重复执行（同一路径只生效一次），避免副作用重复
    ///
    /// 参数：
    ///   - path: import 语句中的路径字面量
    ///   - cur_file: 当前正在执行的脚本文件名（用于解析相对路径基准）
    pub fn do_import(&mut self, path: &str, cur_file: &str) -> Result<(), Value> {
        // 1. 解析路径：相对路径基于当前脚本目录
        let resolved = resolve_import_path(path, cur_file);

        // 2. 规范化绝对路径（用于环检测与模块缓存）
        let canonical = std::fs::canonicalize(&resolved)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| resolved.clone());

        // 3. 模块缓存：已加载过则跳过（幂等，避免重复执行副作用）
        if self.imported_modules.contains(&canonical) {
            return Ok(());
        }

        // 4. 环检测：路径已在加载栈中 → 循环 import
        if self.import_stack.iter().any(|p| p == &canonical) {
            return Err(error_value(format!(
                "import 循环依赖：'{}' (可能原因：A import B，B 又 import A；请重构消除环)",
                canonical,
            )));
        }

        // 5. 读取文件
        let src = std::fs::read_to_string(&resolved).map_err(|e| {
            let hint = match e.kind() {
                std::io::ErrorKind::NotFound => "模块文件不存在（检查路径或当前工作目录）",
                std::io::ErrorKind::PermissionDenied => "权限不足",
                _ => "路径非法或被占用",
            };
            error_value(format!(
                "import 失败：无法读取 '{}' - {} (可能原因：{})",
                path, e, hint,
            ))
        })?;

        // 6. 编译：lex → parse → compile
        let tokens = tokenize(&src, &canonical).map_err(|e| {
            error_value(format!("import '{}' 词法错误: {}", path, e))
        })?;
        let prog = parse_program(tokens, &canonical).map_err(|e| {
            error_value(format!("import '{}' 语法错误: {}", path, e))
        })?;
        let sub_code = compile(&prog).map_err(|e| {
            error_value(format!("import '{}' 编译错误: {}", path, e))
        })?;

        // 7. 执行：压入加载栈，递归执行子脚本（共享 self.globals）
        self.import_stack.push(canonical.clone());
        let result = self.run(Arc::new(sub_code));
        self.import_stack.pop();
        // 仅在执行成功时标记为已加载（幂等）。
        // 失败时不标记：这样错误恢复/重试流程可重新加载（修正后的脚本会重新执行）。
        match &result {
            Ok(_) => {
                self.imported_modules.push(canonical);
                Ok(())
            }
            Err(e) => Err(e.clone()),
        }
    }

    /// compound_index 执行 a[i] op= v，返回新值（op 由 flag 编码）。
    /// flag 低 4 位为运算类型索引，与 compound_op 解码对应。
    fn compound_index(obj: &Value, idx: &Value, v: Value, flag: u8) -> Result<Value, String> {
        // 读取旧值
        let old = index_get(obj, idx)?;
        // ??= 特殊：仅当 old 为 undefined 才赋值（返回新值），否则返回 old（不赋值）
        if flag & 0x0f == 0x05 {
            if matches!(old, Value::Undefined) {
                index_set(obj, idx, v.clone())?;
                return Ok(v);
            }
            return Ok(old);
        }
        let op = compound_op(flag & 0x0f)?;
        let new = arith_op(op, old, v)?;
        index_set(obj, idx, new.clone())?;
        Ok(new)
    }

    /// compound_member 执行 obj.k op= v，返回新值。
    fn compound_member(obj: &Value, name: &str, v: Value, flag: u8) -> Result<Value, String> {
        let old = member_get(obj, name)?;
        if flag & 0x0f == 0x05 {
            // ??=
            if matches!(old, Value::Undefined) {
                member_set(obj, name, v.clone())?;
                return Ok(v);
            }
            return Ok(old);
        }
        let op = compound_op(flag & 0x0f)?;
        let new = arith_op(op, old, v)?;
        member_set(obj, name, new.clone())?;
        Ok(new)
    }

    /// spawn_thread 启动新任务执行函数调用（`run` 关键字；阶段三起为调度器任务）。
    ///
    /// 设计（goroutine 式轻量并发）：
    ///   - run 不再启动 OS 线程，而是向调度器入队一个任务（创建成本 ~百 ns 级、
    ///     内存 ~KB 级，对比 OS 线程 ~17µs / ~50KB+8MB 栈预留）
    ///   - 任务 VM 共享 self.globals 与 self.out：var/func 定义互通、输出汇聚
    ///   - 阻塞类操作（chanRecv/lock/sleep 等）在任务内挂起而非阻塞 OS 线程
    ///   - 异常在任务结束时打印（Error 与非 Error 的 throw 值都打印），不传播
    fn spawn_thread(&self, callee: Value, args: Vec<Value>) {
        let globals = self.globals.clone();
        let out = self.out.clone();
        if let Err(e) = crate::scheduler::spawn_task(callee, args, globals, out) {
            // 任务创建失败（如目标非函数）：打印提示而非静默丢弃
            let msg = match &e {
                Value::Error(x) => x.message.clone(),
                other => other.to_str(),
            };
            let _ = writeln!(self.out.lock().unwrap(), "[run 任务启动失败] {}", msg);
        }
    }

    /// expand_spread_args 弹出 argc 个参数并按 spread_mask 展开数组。
    ///
    /// 栈布局：[..., arg0, arg1, ..., argN]（argN 在顶）。bit i 为 1 表示第 i 个
    /// 参数是数组，展开为逐个元素。返回展开后的参数列表（保持顺序）。
    fn expand_spread_args(stack: &mut Vec<Value>, argc: usize, spread_mask: u64) -> Result<Vec<Value>, String> {
        let mut all_args: Vec<Value> = Vec::new();
        // 从后往前弹（栈顶是最后一个参数），插入到头部保持顺序
        for i in (0..argc).rev() {
            let v = stack.pop().expect("stack underflow");
            if spread_mask & (1u64 << i) != 0 {
                match &v {
                    Value::Array(a) => {
                        let elements = a.lock().unwrap().clone();
                        for e in elements.into_iter().rev() {
                            all_args.insert(0, e);
                        }
                    }
                    _ => {
                        // 非数组无法展开：记录类型名，把已弹出的参数压回，保持栈一致后报错
                        let tn = v.type_name();
                        for a in all_args.into_iter().rev() {
                            stack.push(a);
                        }
                        stack.push(v);
                        return Err(format!(
                            "无法展开非数组类型 {} (可能原因：... 只能用于数组)", tn
                        ));
                    }
                }
            } else {
                all_args.insert(0, v);
            }
        }
        Ok(all_args)
    }

    /// incdec_index 索引自增自减：a[i]±1，返回 (旧值, 新值)。
    /// 新值经 arith_op 计算（Int/Float/BigInt 均正确），写入后返回。
    fn incdec_index(obj: &Value, idx: &Value, inc: bool) -> Result<(Value, Value), String> {
        let old = index_get(obj, idx)?;
        let op = if inc { Opcode::Add } else { Opcode::Sub };
        let new = arith_op(op, old.clone(), Value::Int(1))?;
        index_set(obj, idx, new.clone())?;
        Ok((old, new))
    }

    /// incdec_member 成员自增自减：obj.k±1，返回 (旧值, 新值)。
    fn incdec_member(obj: &Value, name: &str, inc: bool) -> Result<(Value, Value), String> {
        let old = member_get(obj, name)?;
        let op = if inc { Opcode::Add } else { Opcode::Sub };
        let new = arith_op(op, old.clone(), Value::Int(1))?;
        member_set(obj, name, new.clone())?;
        Ok((old, new))
    }
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

/// compound_op 将复合赋值的 flag（低 4 位）映射为对应的算术 opcode。
/// 编码：0=Add 1=Sub 2=Mul 3=Div 4=Mod 5=NullCoal(??=, 调用处特判) 6=BitAnd 7=BitOr 8=BitXor 9=Shl 10=Shr
fn compound_op(flag: u8) -> Result<Opcode, String> {
    match flag {
        0 => Ok(Opcode::Add),
        1 => Ok(Opcode::Sub),
        2 => Ok(Opcode::Mul),
        3 => Ok(Opcode::Div),
        4 => Ok(Opcode::Mod),
        // 5 = ??=，已在 compound_index/member 处特判，不应到达此处
        6 => Ok(Opcode::BitAnd),
        7 => Ok(Opcode::BitOr),
        8 => Ok(Opcode::BitXor),
        9 => Ok(Opcode::BitShl),
        10 => Ok(Opcode::BitShr),
        other => Err(format!("invalid compound op flag: {}", other)),
    }
}

/// arith_op 算术与位运算。
fn arith_op(op: Opcode, a: Value, b: Value) -> Result<Value, String> {
    // 字符串拼接（仅 +）
    if op == Opcode::Add {
        if let (Value::Str(s1), Value::Str(s2)) = (&a, &b) {
            let mut s = String::with_capacity(s1.len() + s2.len());
            s.push_str(s1);
            s.push_str(s2);
            return Ok(Value::Str(Arc::from(s.as_str())));
        }
        // 一侧是字符串、另一侧非字符串 → 自动 to_str 拼接
        // （int+int 等纯数值运算不匹配此处，仍走下面的数值分支）
        if matches!(&a, Value::Str(_)) || matches!(&b, Value::Str(_)) {
            let sa = a.to_str();
            let sb = b.to_str();
            return Ok(Value::Str(Arc::from((sa + &sb).as_str())));
        }
    }
    // 位运算：仅整数参与（Float 报错，类型不兼容）
    match op {
        Opcode::BitAnd | Opcode::BitOr | Opcode::BitXor | Opcode::BitShl | Opcode::BitShr => {
            return bit_op(op, &a, &b);
        }
        _ => {}
    }
    // ---- byte 运算 ----
    // Byte op Byte → Byte（算术 mod 256 环绕；位运算结果必在 0-255）
    match (&a, &b) {
        (Value::Byte(x), Value::Byte(y)) => {
            let r: u8 = match op {
                Opcode::Add => x.wrapping_add(*y),
                Opcode::Sub => x.wrapping_sub(*y),
                Opcode::Mul => x.wrapping_mul(*y),
                // 除法/取模结果可能不在 byte 范围语义内，提升为 int
                Opcode::Div => {
                    if *y == 0 { return Err("division by zero (除零错误)".into()); }
                    return Ok(Value::Int((*x / *y) as i64));
                }
                Opcode::Mod => {
                    if *y == 0 { return Err("modulo by zero".into()); }
                    return Ok(Value::Int((*x % *y) as i64));
                }
                _ => unreachable!(),
            };
            return Ok(Value::Byte(r));
        }
        // Byte + Int → Int（byte 提升为 int）
        (Value::Byte(x), Value::Int(y)) => {
            return arith_op(op, Value::Int(*x as i64), Value::Int(*y));
        }
        (Value::Int(x), Value::Byte(y)) => {
            return arith_op(op, Value::Int(*x), Value::Int(*y as i64));
        }
        // Byte + Float → Float（byte 提升为 float）
        (Value::Byte(x), Value::Float(y)) => {
            return arith_op(op, Value::Float(*x as f64), Value::Float(*y));
        }
        (Value::Float(x), Value::Byte(y)) => {
            return arith_op(op, Value::Float(*x), Value::Float(*y as f64));
        }
        _ => {}
    }
    // ---- 原有数值运算 ----
    match (&a, &b) {
        (Value::Int(x), Value::Int(y)) => {
            let r = match op {
                Opcode::Add => x.wrapping_add(*y),
                Opcode::Sub => x.wrapping_sub(*y),
                Opcode::Mul => x.wrapping_mul(*y),
                Opcode::Div => {
                    if *y == 0 { return Err("division by zero (除零错误；可能原因：除数为 0)".into()); }
                    x.wrapping_div(*y)
                }
                Opcode::Mod => {
                    if *y == 0 { return Err("modulo by zero".into()); }
                    x.wrapping_rem(*y)
                }
                _ => unreachable!(),
            };
            Ok(Value::Int(r))
        }
        (Value::Float(x), Value::Float(y)) => {
            let r = match op {
                Opcode::Add => x + y,
                Opcode::Sub => x - y,
                Opcode::Mul => x * y,
                Opcode::Div => x / y,
                Opcode::Mod => x % y,
                _ => unreachable!(),
            };
            Ok(Value::Float(r))
        }
        (Value::Int(x), Value::Float(y)) => arith_op(op, Value::Float(*x as f64), Value::Float(*y)),
        (Value::Float(x), Value::Int(y)) => arith_op(op, Value::Float(*x), Value::Float(*y as f64)),
        // ---- BigInt 互通 ----
        // 注意：非交换运算（- / %）必须保持操作数顺序，故拆分为独立 arm
        (Value::BigInt(a), Value::BigInt(b)) => big_arith(op, a, b),
        (Value::Int(x), Value::BigInt(b)) => {
            let a_bi = std::sync::Arc::new(crate::bigint::BigInt::from_i64(*x));
            big_arith(op, &a_bi, b)
        }
        (Value::BigInt(b), Value::Int(x)) => {
            let b_bi = std::sync::Arc::new(crate::bigint::BigInt::from_i64(*x));
            big_arith(op, b, &b_bi)
        }
        // ---- BigFloat 互通 ----
        (Value::BigFloat(a), Value::BigFloat(b)) => bigfloat_arith(op, a, b),
        (Value::BigInt(a), Value::BigFloat(b)) => {
            let a_bf = std::sync::Arc::new(crate::bigfloat::BigFloat::from_bigint((**a).clone()));
            bigfloat_arith(op, &a_bf, b)
        }
        (Value::BigFloat(b), Value::BigInt(a)) => {
            let a_bf = std::sync::Arc::new(crate::bigfloat::BigFloat::from_bigint((**a).clone()));
            bigfloat_arith(op, b, &a_bf)
        }
        (Value::Int(x), Value::BigFloat(b)) => {
            let a_bf = std::sync::Arc::new(crate::bigfloat::BigFloat::from_i64(*x));
            bigfloat_arith(op, &a_bf, b)
        }
        (Value::BigFloat(b), Value::Int(x)) => {
            let b_bf = std::sync::Arc::new(crate::bigfloat::BigFloat::from_i64(*x));
            bigfloat_arith(op, b, &b_bf)
        }
        // BigInt/BigFloat 与 Float 混算：报错（精度语义冲突，需用户显式转换）
        (Value::Float(_), Value::BigInt(_)) | (Value::BigInt(_), Value::Float(_))
        | (Value::Float(_), Value::BigFloat(_)) | (Value::BigFloat(_), Value::Float(_)) => {
            Err(format!("cannot {:?} {} and {} (可能原因：大数(bigInt/bigFloat)不与 float 直接混算，请先转换)", op, a.type_name(), b.type_name()))
        }
        _ => Err(format!("cannot {:?} {} and {} (可能原因：类型不匹配；算术运算要求数值或字符串)", op, a.type_name(), b.type_name())),
    }
}

/// bit_op 位运算（仅整数 i64；Float/其他类型报错）。
fn bit_op(op: Opcode, a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            let r = match op {
                Opcode::BitAnd => x & y,
                Opcode::BitOr => x | y,
                Opcode::BitXor => x ^ y,
                Opcode::BitShl => x.wrapping_shl(*y as u32),
                Opcode::BitShr => x.wrapping_shr(*y as u32),
                _ => unreachable!(),
            };
            Ok(Value::Int(r))
        }
        // Byte op Byte 位运算 → Byte（& | ^ 结果必在 0-255；移位提升为 int）
        (Value::Byte(x), Value::Byte(y)) => {
            match op {
                Opcode::BitAnd => Ok(Value::Byte(x & y)),
                Opcode::BitOr => Ok(Value::Byte(x | y)),
                Opcode::BitXor => Ok(Value::Byte(x ^ y)),
                // 移位可能超出 byte 范围，提升为 int
                Opcode::BitShl => Ok(Value::Int((*x as i64).wrapping_shl(*y as u32))),
                Opcode::BitShr => Ok(Value::Int((*x as i64).wrapping_shr(*y as u32))),
                _ => unreachable!(),
            }
        }
        // Byte op Int / Int op Byte 位运算 → Int（byte 提升）
        (Value::Byte(x), Value::Int(y)) => bit_op(op, &Value::Int(*x as i64), &Value::Int(*y)),
        (Value::Int(x), Value::Byte(y)) => bit_op(op, &Value::Int(*x), &Value::Int(*y as i64)),
        _ => Err(format!(
            "cannot {:?} {} and {} (可能原因：位运算仅支持整数/字节；浮点/其他类型不兼容)",
            op, a.type_name(), b.type_name(),
        )),
    }
}

/// big_arith BigInt 算术（加/减/乘/除/模）。
///
/// 结果若能装回 i64 则降级为 Int（避免小结果仍用 BigInt）；否则保持 BigInt。
fn big_arith(op: Opcode, a: &std::sync::Arc<crate::bigint::BigInt>, b: &std::sync::Arc<crate::bigint::BigInt>) -> Result<Value, String> {
    use crate::bigint::BigInt;
    let result: BigInt = match op {
        Opcode::Add => a.add(b),
        Opcode::Sub => a.sub(b),
        Opcode::Mul => a.mul(b),
        Opcode::Div => {
            let (q, _r) = a.divmod(b)?;
            q
        }
        Opcode::Mod => {
            let (_q, r) = a.divmod(b)?;
            r
        }
        _ => return Err(format!("bigInt 不支持运算 {:?}", op)),
    };
    // 能装回 i64 则降级为 Int（小结果用更高效的 Int 表示）
    match result.to_i64() {
        Some(i) => Ok(Value::Int(i)),
        None => Ok(Value::BigInt(std::sync::Arc::new(result))),
    }
}

/// bigfloat_arith BigFloat 算术。
///
/// 除法默认 20 位小数（可用 bigFloatDiv 指定更高精度）。
fn bigfloat_arith(op: Opcode, a: &std::sync::Arc<crate::bigfloat::BigFloat>, b: &std::sync::Arc<crate::bigfloat::BigFloat>) -> Result<Value, String> {
    use crate::bigfloat::BigFloat;
    let result: BigFloat = match op {
        Opcode::Add => a.add(b),
        Opcode::Sub => a.sub(b),
        Opcode::Mul => a.mul(b),
        Opcode::Div => a.div_default(b)?,
        Opcode::Mod => return Err("bigFloat 不支持取模 % (可能原因：浮点无整数取模语义)".into()),
        _ => return Err(format!("bigFloat 不支持运算 {:?}", op)),
    };
    Ok(Value::BigFloat(std::sync::Arc::new(result)))
}

/// cmp_op 比较运算。
/// cmp_apply 将 Ordering + Opcode 转为布尔比较结果。
fn cmp_apply(op: Opcode, ord: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op {
        Opcode::LT => ord == Less,
        Opcode::LE => ord != Greater,
        Opcode::GT => ord == Greater,
        Opcode::GE => ord != Less,
        _ => unreachable!(),
    }
}

fn cmp_op(op: Opcode, a: Value, b: Value) -> Result<Value, String> {
    let r = match (&a, &b) {
        (Value::Int(x), Value::Int(y)) => match op {
            Opcode::LT => x < y,
            Opcode::LE => x <= y,
            Opcode::GT => x > y,
            Opcode::GE => x >= y,
            _ => unreachable!(),
        },
        // Byte 比较（Byte-Byte / Byte-Int / Int-Byte，跨类型按值）
        (Value::Byte(x), Value::Byte(y)) => cmp_apply(op, (*x as i64).cmp(&(*y as i64))),
        (Value::Byte(x), Value::Int(y)) => cmp_apply(op, (*x as i64).cmp(y)),
        (Value::Int(x), Value::Byte(y)) => cmp_apply(op, x.cmp(&(*y as i64))),
        (Value::Float(x), Value::Float(y)) => match op {
            Opcode::LT => x < y,
            Opcode::LE => x <= y,
            Opcode::GT => x > y,
            Opcode::GE => x >= y,
            _ => unreachable!(),
        },
        (Value::Int(x), Value::Float(y)) => cmp_apply(op, (*x as f64).partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)),
        (Value::Float(x), Value::Int(y)) => cmp_apply(op, x.partial_cmp(&(*y as f64)).unwrap_or(std::cmp::Ordering::Equal)),
        // ---- BigInt/BigFloat 跨类型比较 ----
        (Value::BigInt(a), Value::BigInt(b)) => cmp_apply(op, a.cmp(b)),
        (Value::Int(x), Value::BigInt(b)) => {
            cmp_apply(op, crate::bigint::BigInt::from_i64(*x).cmp(b))
        }
        (Value::BigInt(b), Value::Int(x)) => {
            cmp_apply(op, b.cmp(&crate::bigint::BigInt::from_i64(*x)))
        }
        (Value::BigFloat(a), Value::BigFloat(b)) => cmp_apply(op, a.cmp(b)),
        (Value::Int(x), Value::BigFloat(b)) => {
            cmp_apply(op, crate::bigfloat::BigFloat::from_i64(*x).cmp(b))
        }
        (Value::BigFloat(b), Value::Int(x)) => {
            // b OP x，需比较 b 与 x（不是 x 与 b）
            cmp_apply(op, b.cmp(&crate::bigfloat::BigFloat::from_i64(*x)))
        }
        (Value::BigInt(a), Value::BigFloat(b)) => {
            cmp_apply(op, crate::bigfloat::BigFloat::from_bigint((**a).clone()).cmp(b))
        }
        (Value::BigFloat(b), Value::BigInt(a)) => {
            // b OP a，需比较 b 与 a（不是 a 与 b）
            cmp_apply(op, b.cmp(&crate::bigfloat::BigFloat::from_bigint((**a).clone())))
        }
        (Value::Str(x), Value::Str(y)) => match op {
            Opcode::LT => x < y,
            Opcode::LE => x <= y,
            Opcode::GT => x > y,
            Opcode::GE => x >= y,
            _ => unreachable!(),
        },
        _ => return Err(format!("cannot compare {} and {} (可能原因：类型不匹配)", a.type_name(), b.type_name())),
    };
    Ok(Value::Bool(r))
}

/// index_get 索引读取 a[i]。
fn index_get(obj: &Value, idx: &Value) -> Result<Value, String> {
    match (obj, idx) {
        (Value::Array(a), Value::Int(i)) => {
            let arr = a.lock().unwrap();
            let n = arr.len() as i64;
            let i = if *i < 0 { *i + n } else { *i };
            if i < 0 || i >= n {
                return Err(format!("array index out of range: {} (len={}); 可能原因：索引越界", i, n));
            }
            Ok(arr[i as usize].clone())
        }
        (Value::Str(s), Value::Int(i)) => {
            // string[i] 返回第 i 个字符的 Unicode 码点（int），按字符索引（不切断多字节）。
            // 与 charFromCode(n) 配对：charFromCode(s[i]) == 原字符。
            let n = s.chars().count() as i64;
            let i = if *i < 0 { *i + n } else { *i };
            if i < 0 || i >= n {
                return Err(format!("string index out of range: {} (len={}); 可能原因：索引越界", i, n));
            }
            let code = s.chars().nth(i as usize).unwrap() as u32 as i64;
            Ok(Value::Int(code))
        }
        (Value::Object(o), Value::Str(k)) => {
            Ok(o.lock().unwrap().get_proto(k).unwrap_or(Value::Undefined))
        }
        (Value::Map(m), Value::Str(k)) => {
            Ok(m.lock().unwrap().get(k).unwrap_or(Value::Undefined))
        }
        (Value::Bytes(b), Value::Int(i)) => {
            let arr = b.as_ref();
            let n = arr.len() as i64;
            let i = if *i < 0 { *i + n } else { *i };
            if i < 0 || i >= n {
                return Err(format!("bytes index out of range: {}", i));
            }
            Ok(Value::Byte(arr[i as usize]))
        }
        (Value::ByteArray(b), Value::Int(i)) => {
            // 可变字节序列读：返回 Byte
            let arr = b.lock().unwrap();
            let n = arr.len() as i64;
            let i = if *i < 0 { *i + n } else { *i };
            if i < 0 || i >= n {
                return Err(format!("byteArray index out of range: {} (len={}); 可能原因：索引越界", i, n));
            }
            Ok(Value::Byte(arr[i as usize]))
        }
        _ => Err(format!("cannot index {} with {} (可能原因：类型不匹配；数组用整数索引，对象用字符串键)", obj.type_name(), idx.type_name())),
    }
}

/// index_set 索引设置 a[i] = v。
fn index_set(obj: &Value, idx: &Value, v: Value) -> Result<(), String> {
    match (obj, idx) {
        (Value::Array(a), Value::Int(i)) => {
            let mut arr = a.lock().unwrap();
            let n = arr.len() as i64;
            let i = if *i < 0 { *i + n } else { *i };
            if i < 0 || i >= n {
                return Err(format!("array index out of range: {} (len={})", i, n));
            }
            arr[i as usize] = v;
            Ok(())
        }
        (Value::Object(o), Value::Str(k)) => {
            o.lock().unwrap().set((*k).to_string(), v);
            Ok(())
        }
        (Value::Map(m), Value::Str(k)) => {
            m.lock().unwrap().set((*k).to_string(), v);
            Ok(())
        }
        (Value::ByteArray(b), Value::Int(i)) => {
            // 可变字节序列写：就地修改。值须为 Int 且 0-255。
            let byte_val = match v {
                Value::Byte(x) => x,
                Value::Int(x) => {
                    if x < 0 || x > 255 {
                        return Err(format!(
                            "byteArray 赋值超出字节范围: {} (需 0-255；可能原因：传入了非字节整数)",
                            x,
                        ));
                    }
                    x as u8
                }
                _ => return Err(format!(
                    "byteArray 赋值需要 byte 或 int 字节值 (0-255)，得到 {} (可能原因：类型不匹配)",
                    v.type_name(),
                )),
            };
            let mut arr = b.lock().unwrap();
            let n = arr.len() as i64;
            let i = if *i < 0 { *i + n } else { *i };
            if i < 0 || i >= n {
                return Err(format!("byteArray index out of range: {} (len={})", i, n));
            }
            arr[i as usize] = byte_val;
            Ok(())
        }
        _ => Err(format!("cannot set index on {} with {} (可能原因：类型不匹配)", obj.type_name(), idx.type_name())),
    }
}

/// slice_value 切片 a[low:high]，按类型分发单位与返回类型。
///
/// - string：按字符切片（不切断多字节字符），返回 string
/// - array：按元素切片，返回 array
/// - bytes：按字节切片，返回 bytes
/// - byteArray：按字节切片，返回 byteArray（类型一致，便于后续就地修改）
///
/// low/high 缺省（None）表示到边界（0 / len）。支持负索引（从尾算）。
/// low >= high 返回空（与 Python/Go 一致）。
fn slice_value(obj: &Value, low: Option<i64>, high: Option<i64>) -> Result<Value, String> {
    /// norm 将 low/high 归一化为 [0, len] 内的 usize，支持负索引与缺省。
    ///
    /// 负索引溢出（如 -100 对长度 3）clamp 到 0（对齐 Python/JS 的宽容语义，
    /// 不报错——脚本语言不应因索引偏大而崩）。
    fn norm(v: Option<i64>, len: i64, which: &str) -> Result<usize, String> {
        match v {
            None => Ok(if which == "low" { 0 } else { len as usize }),
            Some(i) => {
                let i = if i < 0 { i + len } else { i };
                // 负索引溢出 clamp 到 0；上界超过 len 截断到 len（到尾）
                let clamped = if i < 0 { 0 } else if i > len { len } else { i };
                Ok(clamped as usize)
            }
        }
    }
    match obj {
        Value::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            let n = chars.len() as i64;
            let lo = norm(low, n, "low")?;
            let hi = norm(high, n, "high")?;
            if lo >= hi {
                return Ok(Value::str(""));
            }
            let part: String = chars[lo..hi].iter().collect();
            Ok(Value::str_from(part))
        }
        Value::Array(a) => {
            let guard = a.lock().unwrap();
            let n = guard.len() as i64;
            let lo = norm(low, n, "low")?;
            let hi = norm(high, n, "high")?;
            if lo >= hi {
                return Ok(Value::Array(Arc::new(Mutex::new(Vec::new()))));
            }
            let part = guard[lo..hi].to_vec();
            Ok(Value::Array(Arc::new(Mutex::new(part))))
        }
        Value::Bytes(b) => {
            let n = b.len() as i64;
            let lo = norm(low, n, "low")?;
            let hi = norm(high, n, "high")?;
            if lo >= hi {
                return Ok(Value::Bytes(Arc::new(Vec::new())));
            }
            let part = b[lo..hi].to_vec();
            Ok(Value::Bytes(Arc::new(part)))
        }
        Value::ByteArray(b) => {
            let guard = b.lock().unwrap();
            let n = guard.len() as i64;
            let lo = norm(low, n, "low")?;
            let hi = norm(high, n, "high")?;
            if lo >= hi {
                return Ok(Value::ByteArray(Arc::new(Mutex::new(Vec::new()))));
            }
            let part = guard[lo..hi].to_vec();
            Ok(Value::ByteArray(Arc::new(Mutex::new(part))))
        }
        _ => Err(format!("cannot slice {} (可能原因：仅 string/array/bytes/byteArray 支持切片)", obj.type_name())),
    }
}
/// member_get 成员读取 a.name（沿原型链）。
fn member_get(obj: &Value, name: &str) -> Result<Value, String> {
    match obj {
        Value::Object(o) => Ok(o.lock().unwrap().get_proto(name).unwrap_or(Value::Undefined)),
        Value::Array(a) => {
            // 数组内置成员：len
            match name {
                "len" => Ok(Value::Int(a.lock().unwrap().len() as i64)),
                _ => Err(format!("array has no member '{}' (可能原因：成员名错误；数组支持 .len)", name)),
            }
        }
        Value::Str(s) => {
            match name {
                "len" => Ok(Value::Int(s.chars().count() as i64)),
                _ => Err(format!("string has no member '{}' (可能原因：成员名错误；字符串支持 .len)", name)),
            }
        }
        Value::ByteArray(b) => {
            match name {
                "len" => Ok(Value::Int(b.lock().unwrap().len() as i64)),
                _ => Err(format!("byteArray has no member '{}' (可能原因：成员名错误；byteArray 支持 .len)", name)),
            }
        }
        Value::DateTime(dt) => {
            match name {
                "year" => Ok(Value::Int(dt.year() as i64)),
                "month" => Ok(Value::Int(dt.month() as i64)),
                "day" => Ok(Value::Int(dt.day() as i64)),
                "hour" => Ok(Value::Int(dt.hour() as i64)),
                "minute" => Ok(Value::Int(dt.minute() as i64)),
                "second" => Ok(Value::Int(dt.second() as i64)),
                "millis" => Ok(Value::Int(dt.millis_part() as i64)),
                "weekday" => Ok(Value::Int(dt.weekday() as i64)),
                "tzOffset" => Ok(Value::Int(dt.tz_offset as i64)),
                _ => Err(format!("datetime has no member '{}' (可能原因：成员名错误)", name)),
            }
        }
        Value::Map(m) => {
            match name {
                "len" => Ok(Value::Int(m.lock().unwrap().len() as i64)),
                _ => Err(format!("map has no member '{}' (可能原因：成员名错误；map 支持 .len)", name)),
            }
        }
        _ => Err(format!("cannot get member '{}' from {} (可能原因：类型不支持成员访问)", name, obj.type_name())),
    }
}

/// member_set 成员设置 a.name = v。
fn member_set(obj: &Value, name: &str, v: Value) -> Result<(), String> {
    match obj {
        Value::Object(o) => {
            o.lock().unwrap().set(name.to_string(), v);
            Ok(())
        }
        _ => Err(format!("cannot set member '{}' on {} (可能原因：仅 object 类型支持成员设置)", name, obj.type_name())),
    }
}

/// resolve_import_path 解析 import 路径为文件系统路径。
///
/// 规则：
///   - 绝对路径（如 `/x/y.sf` 或 `D:\x\y.sf`）直接使用
///   - 相对路径基于当前脚本（cur_file）所在目录解析
///   - cur_file 为 "<string>" / "<run>" 等占位符时，回退到当前工作目录
fn resolve_import_path(path: &str, cur_file: &str) -> String {
    use std::path::{Path, PathBuf};
    let p = Path::new(path);
    // 绝对路径直接使用
    if p.is_absolute() {
        return path.to_string();
    }
    // 相对路径：基于当前脚本目录
    let base = Path::new(cur_file).parent();
    match base {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(path).to_string_lossy().into_owned(),
        // cur_file 无目录部分（如 "<string>"）：用当前工作目录
        _ => {
            let cwd: PathBuf = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            cwd.join(path).to_string_lossy().into_owned()
        }
    }
}

/// deref_value 解引用：从 Native(Arc<Mutex<Value>>) 包装中读取内部值。
fn deref_value(v: &Value) -> Result<Value, String> {
    use std::sync::Arc;
    use std::sync::Mutex;
    match v {
        Value::Native(n) => {
            if let Some(cell) = n.downcast_ref::<Arc<Mutex<Value>>>() {
                Ok(cell.lock().unwrap().clone())
            } else {
                Err("cannot dereference non-ref value".into())
            }
        }
        _ => Err(format!("cannot dereference {} (可能原因：只有 & 创建的引用才能用 * 解引用)", v.type_name())),
    }
}

/// set_deref_value 引用赋值：写入 Native(Arc<Mutex<Value>>) 包装。
fn set_deref_value(ref_val: &Value, new_val: Value) -> Result<(), String> {
    use std::sync::Arc;
    use std::sync::Mutex;
    match ref_val {
        Value::Native(n) => {
            if let Some(cell) = n.downcast_ref::<Arc<Mutex<Value>>>() {
                *cell.lock().unwrap() = new_val;
                Ok(())
            } else {
                Err("cannot set deref: not a ref wrapper".into())
            }
        }
        _ => Err(format!("cannot set deref on {} (可能原因：只有 & 创建的引用才能赋值)", ref_val.type_name())),
    }
}
