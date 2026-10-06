//! conc_task_test.rs — 任务调度器端到端测试（goroutine 式并发，阶段3+4）
//!
//! 覆盖点：
//!   - 海量任务：万级任务 spawn+完成（旧 OS 线程模型的万级阻塞任务不可行）
//!   - 注入式唤醒：任务先挂起在 chanRecv，主线程后发送（值注入到达任务）
//!   - 等待-重试型：wgWait/semAcquire 挂起后被唤醒重查
//!   - sleep 在任务内挂起（定时器唤醒），大量并发 sleep 不占 OS 线程
//!   - 交接式 mutex：多任务并发计数正确
//!   - onceDo 跨任务：等待者直接取得首次执行结果
//!   - park 限制：回调内挂起返回明确错误

use std::time::{Duration, Instant};

use sflang::Sflang;
use sflang::value::Value;

/// run_with_timeout 在独立线程执行脚本，超时视为死锁并使测试失败。
/// 接收 String（支持 format! 动态构建的脚本，如含本地服务地址的用例）。
fn run_with_timeout(src: String, timeout: Duration) -> Value {
    let (tx, rx) = std::sync::mpsc::channel();
    let script = src.clone(); // 闭包与超时提示各持一份
    std::thread::spawn(move || {
        let mut sf = Sflang::new();
        let _ = tx.send(sf.run_string(&script));
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => panic!("脚本执行返回错误: {:?}", e),
        Err(_) => panic!("脚本执行超时（疑似死锁）: {}", src),
    }
}

/// test_10k_tasks_spawn_complete 万级任务 spawn + channel 收集（旧线程模型不可行）。
#[test]
fn test_10k_tasks_spawn_complete() {
    let src = r#"
var ch = newChannel()
func sender() {
    chanSend(ch, 1)
}
for i in range(10000) {
    run sender()
}
var total = 0
for i in range(10000) {
    total = total + chanRecv(ch)
}
return total
"#;
    let r = run_with_timeout(src.to_string(), Duration::from_secs(120));
    assert_eq!(r, Value::Int(10000), "万级任务全部完成并各发送一次");
}

/// test_inject_wake_after_park 注入式唤醒：任务先挂起，主线程后发送。
#[test]
fn test_inject_wake_after_park() {
    // ready 通道两步握手：任务到挂起点后主线程再发送，保证挂起先于发送
    let src = r#"
var ready = newChannel()
var data = newChannel()
func waiter() {
    chanSend(ready, 1)
    var v = chanRecv(data)   // 挂起等待主线程发送
    chanSend(ready, v * 10)
}
run waiter()
chanRecv(ready)              // 任务已到达挂起点（或即将到达）
sleepMs(100)                 // 给任务足够时间进入挂起（保证先挂起后发送）
chanSend(data, 7)
var v2 = chanRecv(ready)     // 任务醒来后发送 7*10
return v2
"#;
    let r = run_with_timeout(src.to_string(), Duration::from_secs(30));
    assert_eq!(r, Value::Int(70), "任务挂起后收到的注入值应参与计算");
}

/// test_sleep_tasks_suspend 大量并发 sleep：任务挂起不占线程，总耗时≈单次时长。
#[test]
fn test_sleep_tasks_suspend() {
    // 500 个任务各睡 300ms：若真开 500 个线程或串行阻塞都会显著更慢/更重；
    // 挂起模型下总耗时约等于单个 sleep 时长 + 调度开销
    let src = r#"
var wg = newWaitGroup()
func sleeper() {
    sleepMs(300)
    wgDone(wg)
}
wgAdd(wg, 500)
for i in range(500) {
    run sleeper()
}
wgWait(wg)
return 1
"#;
    let t0 = Instant::now();
    let r = run_with_timeout(src.to_string(), Duration::from_secs(60));
    let elapsed = t0.elapsed();
    assert_eq!(r, Value::Int(1));
    assert!(
        elapsed < Duration::from_millis(5000),
        "500 个 300ms 并发 sleep 应在 5s 内完成（挂起模型），实际 {:?}",
        elapsed
    );
}

/// test_mutex_counter_500_tasks 交接式 mutex：500 任务并发计数正确。
#[test]
fn test_mutex_counter_500_tasks() {
    let src = r#"
var mu = newMutex()
var counter = 0
var wg = newWaitGroup()
func worker() {
    for j in range(100) {
        lock(mu)
        counter = counter + 1
        unlock(mu)
    }
    wgDone(wg)
}
wgAdd(wg, 500)
for i in range(500) {
    run worker()
}
wgWait(wg)
return counter
"#;
    let r = run_with_timeout(src.to_string(), Duration::from_secs(60));
    assert_eq!(r, Value::Int(50000), "500 任务 × 100 次并发计数应精确");
}

/// test_once_across_tasks onceDo 跨任务：并发等待者取得首次执行结果。
#[test]
fn test_once_across_tasks() {
    let src = r#"
var once = newOnce()
var wg = newWaitGroup()
var results = []
func caller() {
    var v = onceDo(once, initFn)
    push(results, v)
    wgDone(wg)
}
func initFn() {
    return 42
}
wgAdd(wg, 100)
for i in range(100) {
    run caller()
}
wgWait(wg)
return len(results)
"#;
    let r = run_with_timeout(src.to_string(), Duration::from_secs(60));
    assert_eq!(r, Value::Int(100), "100 个并发 onceDo 全部返回");
}

/// test_semaphore_limits_task_concurrency 信号量限流：并发峰值不超过许可数。
#[test]
fn test_semaphore_limits_task_concurrency() {
    let src = r#"
var sem = newSemaphore(3)
var mu = newMutex()
var cur = 0
var peak = 0
var wg = newWaitGroup()
func limited() {
    semAcquire(sem)
    lock(mu)
    cur = cur + 1
    if cur > peak { peak = cur }
    unlock(mu)
    sleepMs(30)
    lock(mu)
    cur = cur - 1
    unlock(mu)
    semRelease(sem)
    wgDone(wg)
}
wgAdd(wg, 50)
for i in range(50) {
    run limited()
}
wgWait(wg)
return peak
"#;
    let r = run_with_timeout(src.to_string(), Duration::from_secs(60));
    match r {
        Value::Int(p) => assert!(p >= 1 && p <= 3, "并发峰值应在 1..=3，实际 {}", p),
        other => panic!("峰值应为 Int，得到 {:?}", other),
    }
}

/// test_park_in_callback_rejected 回调内挂起返回明确错误（不产生状态错乱、不挂死）。
#[test]
fn test_park_in_callback_rejected() {
    // 任务内 onceDo 回调调 chanRecv：park 被禁 → 回调得到明确错误对象而非挂起
    let src = r#"
var once = newOnce()
var ch = newChannel()
var ok = 0
func cb() {
    return chanRecv(ch)   // 回调内挂起 → 任务上下文中应得到错误而非挂死
}
func runner() {
    try {
        onceDo(once, cb)
    } catch (e) {
        ok = 1   // 回调内挂起被拒 → onceDo 传播明确错误（不吞掉、不挂死）
    }
}
run runner()
sleepMs(500)              // 给任务时间执行回调（回调立即得到错误返回）
return ok
"#;
    let r = run_with_timeout(src.to_string(), Duration::from_secs(30));
    assert_eq!(r, Value::Int(1), "回调内挂起应得到错误并正常返回，不挂死");
}

/// test_channel_order_fifo 同一发送者按序发送、接收者按序接收。
#[test]
fn test_channel_order_fifo() {
    let src = r#"
var ch = newChannel()
var seen = []
func producer() {
    for i in range(100) {
        chanSend(ch, i)
    }
}
run producer()
for i in range(100) {
    push(seen, chanRecv(ch))
}
return seen[0] + seen[50] + seen[99]
"#;
    let r = run_with_timeout(src.to_string(), Duration::from_secs(30));
    assert_eq!(r, Value::Int(0 + 50 + 99), "channel 应保持 FIFO 顺序");
}

/// test_rwlock_task_mode 读写锁任务模式：并发读 + 互斥写。
#[test]
fn test_rwlock_task_mode() {
    let src = r#"
var rw = newRWMutex()
var data = 0
var wg = newWaitGroup()
func reader() {
    rlock(rw)
    runlock(rw)
    wgDone(wg)
}
func writer() {
    wlock(rw)
    data = data + 1
    wunlock(rw)
    wgDone(wg)
}
wgAdd(wg, 120)
for i in range(100) {
    run reader()
}
for i in range(20) {
    run writer()
}
wgWait(wg)
return data
"#;
    let r = run_with_timeout(src.to_string(), Duration::from_secs(60));
    assert_eq!(r, Value::Int(20), "20 个写者串行累加，100 个读者并发读");
}

/// test_10k_blocked_channel_tasks 万级任务同时阻塞在 chanRecv（容量验证）。
#[test]
fn test_10k_blocked_channel_tasks() {
    // 10000 个任务全部挂起在 chanRecv，主线程逐个唤醒（旧线程模型：
    // 万级阻塞线程 + 8MB 栈不可行；挂起模型下单 channel 等待队列承载）
    let src = r#"
var ready = newChannel()
var data = newChannel()
var wg = newWaitGroup()
func blocked() {
    chanSend(ready, 1)
    var v = chanRecv(data)
    wgDone(wg)
}
wgAdd(wg, 10000)
for i in range(10000) {
    run blocked()
}
for i in range(10000) {
    chanRecv(ready)      // 等全部任务到达挂起点
}
for i in range(10000) {
    chanSend(data, i)    // 逐个唤醒
}
wgWait(wg)
return 1
"#;
    let r = run_with_timeout(src.to_string(), Duration::from_secs(180));
    assert_eq!(r, Value::Int(1), "万级阻塞任务的挂起与唤醒全部成功");
}

/// test_task_error_printed 任务内异常打印不传播（不挂死主线程）。
#[test]
fn test_task_error_printed() {
    let src = r#"
func boom() {
    throw "task-boom"
}
run boom()
sleepMs(200)
return 1
"#;
    let r = run_with_timeout(src.to_string(), Duration::from_secs(30));
    assert_eq!(r, Value::Int(1));
}

// ---- threadRun 逃生门 + worker 配置（阶段6） ----

/// test_thread_run_shares_globals threadRun 真线程共享全局环境。
#[test]
fn test_thread_run_shares_globals() {
    let src = r#"
var done = newChannel()
func heavy(n) {
    var s = 0
    for i in range(n) {
        s = s + 1
    }
    chanSend(done, s)
}
threadRun(heavy, 1000)
var v = chanRecv(done)
return v
"#;
    let r = run_with_timeout(src.to_string(), Duration::from_secs(30));
    assert_eq!(r, Value::Int(1000), "threadRun 在独立线程执行并共享 globals");
}

/// test_run_named_function_required run 目标必须是函数调用（语法保障）。
#[test]
fn test_run_named_function_required() {
    let mut sf = Sflang::new();
    assert!(
        sf.run_string("run 42").is_err(),
        "run 后非函数调用应报编译错误"
    );
}

// ---- 阻塞型内置函数卸载（异步 IO，任务内不占 worker） ----

use std::io::{Read, Write};

/// 启动本地慢速 HTTP 服务：每个请求 sleep(delay) 后返回 "ok"。
fn spawn_slow_http_server(delay: Duration) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut s = match stream {
                Ok(s) => s,
                Err(_) => break,
            };
            std::thread::spawn(move || {
                // 读完请求头（忽略内容），延迟后响应
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                std::thread::sleep(delay);
                let resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://{}/", addr)
}

/// test_async_io_offload_concurrency 超过 worker 数的任务并发慢 IO：
/// 卸载模型下总耗时 ≈ 单次 IO 时长（内联模型 ≈ 任务数/核数 × 单次时长）。
#[test]
fn test_async_io_offload_concurrency() {
    let base = spawn_slow_http_server(Duration::from_millis(300));
    let n_tasks = 32;
    // 调度 worker = 核数（本机 ≥8）：内联执行 32×300ms 至少 4 轮 ≈ 1.2s+；
    // 卸载后并发跑满阻塞池（256），总耗时 ≈ 300ms + 调度/网络开销
    let src = format!(
        r#"
var wg = newWaitGroup()
func fetcher(i) {{
    var body = getWeb("{base}")
    if isErr(body) {{
        throw "fetch failed: " + body
    }}
    wgDone(wg)
}}
wgAdd(wg, {n})
for i in range({n}) {{
    run fetcher(i)
}}
wgWait(wg)
return 1
"#,
        base = base,
        n = n_tasks
    );
    let t0 = Instant::now();
    let r = run_with_timeout(src.clone(), Duration::from_secs(60));
    let elapsed = t0.elapsed();
    assert_eq!(r, Value::Int(1), "32 个并发 getWeb 全部成功");
    assert!(
        elapsed < Duration::from_millis(2500),
        "卸载模型下 32 个 300ms IO 应远快于内联串行（实际 {:?}）",
        elapsed
    );
}

/// test_async_io_error_thrown_at_call_site 卸载的内置函数失败：
/// 错误以抛出语义在调用点出现，任务内 try/catch 可捕获。
#[test]
fn test_async_io_error_thrown_at_call_site() {
    let src = r#"
var results = []
func reader() {
    try {
        readFile("Z:/definitely/not/exist/file.sf")
        push(results, "no-throw")
    } catch (e) {
        push(results, "caught")
    }
}
run reader()
sleepMs(500)
return len(results)
"#;
    let r = run_with_timeout(src.to_string(), Duration::from_secs(30));
    assert_eq!(r, Value::Int(1), "任务应完成");
    match r {
        Value::Int(1) => {}
        _ => unreachable!(),
    }
    // results 内容经共享数组验证：catch 命中
    let src2 = r#"
var mark = ""
func reader2() {
    try {
        readFile("Z:/definitely/not/exist/file.sf")
        mark = "no-throw"
    } catch (e) {
        mark = "caught"
    }
}
run reader2()
sleepMs(500)
return mark
"#;
    let r2 = run_with_timeout(src2.to_string(), Duration::from_secs(30));
    assert_eq!(r2.to_str(), "caught", "卸载内置函数的错误应以抛出语义到达调用点");
}

/// test_async_io_readfile_value 任务内 readFile 卸载后取得正确结果（值注入）。
#[test]
fn test_async_io_readfile_value() {
    let dir = std::env::temp_dir();
    let path = dir.join("sflang_async_io_test.txt");
    std::fs::write(&path, "hello-async-io").expect("write temp");
    let src = format!(
        r#"
var ch = newChannel()
func reader() {{
    var content = readFile("{}")
    chanSend(ch, content)
}}
run reader()
return chanRecv(ch)
"#,
        path.to_str().unwrap().replace('\\', "/")
    );
    let r = run_with_timeout(src.clone(), Duration::from_secs(30));
    assert_eq!(r.to_str(), "hello-async-io", "卸载 readFile 的结果应经注入送达任务");
    let _ = std::fs::remove_file(&path);
}

/// test_blocking_offload_rejected_in_callback 回调内的阻塞型内置函数不卸载
/// （park 被禁 → 原地执行返回错误对象，不产生状态错乱）。
#[test]
fn test_blocking_offload_rejected_in_callback() {
    let dir = std::env::temp_dir();
    let path = dir.join("sflang_async_io_cb.sf");
    std::fs::write(&path, "x").expect("write temp");
    let src = format!(
        r#"
var once = newOnce()
var mark = ""
func cb() {{
    var v = readFile("{}")
    if isErr(v) {{
        mark = "err"
    }} else {{
        mark = "ok"
    }}
    return v
}}
func runner() {{
    try {{
        onceDo(once, cb)
    }} catch (e) {{
        mark = "callback-park-blocked"
    }}
}}
run runner()
sleepMs(500)
return mark
"#,
        path.to_str().unwrap().replace('\\', "/")
    );
    // 回调内 readFile：readFile 本身不挂起（成功路径无 park），故原地执行成功；
    // 失败（Err）时因 park 被禁不卸载 → Err 直接传播 → onceDo 抛出 → catch
    let r = run_with_timeout(src.clone(), Duration::from_secs(30));
    let m = r.to_str();
    assert!(
        m == "ok" || m == "callback-park-blocked",
        "回调内阻塞内置函数应原地执行成功或得到明确错误，实际: {}",
        m
    );
}
