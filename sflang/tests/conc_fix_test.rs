//! conc_fix_test.rs — 并发原语修复的回归测试
//!
//! 覆盖 concurrency.rs / api.rs 的修复点：
//!   - chanRecv 不再持锁阻塞：多个线程可同时对同一 channel 接收
//!   - RWMutex 单一 Mutex + Condvar 重构后，多读单写压力下不死锁
//!   - onceDo 回调抛错时错误对外返回（不吞掉）；递归调用返回错误而非死锁
//!   - wgAdd 溢出 / 变负返回 error；newSemaphore 非法参数返回 error
//!   - 正常路径回归：mutex 计数、channel 生产消费

use std::time::Duration;

use sflang::Sflang;
use sflang::value::Value;

// ---- 辅助函数 ----

/// run 执行代码，返回 Result（用于断言错误路径）。
fn run(src: &str) -> Result<Value, Value> {
    let mut sf = Sflang::new();
    sf.run_string(src)
}

/// run_with_timeout 在独立线程执行脚本，超时视为死锁并使测试失败。
///
/// 用于可能死锁的并发场景：若实现回退为持锁阻塞/锁序反转，
/// 脚本线程会永远挂起，此处通过 recv_timeout 及时报告而不是让测试挂死。
fn run_with_timeout(src: &'static str, timeout: Duration) -> Value {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut sf = Sflang::new();
        let _ = tx.send(sf.run_string(src));
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => panic!("脚本执行返回错误: {:?}", e),
        Err(_) => panic!("脚本执行超时（疑似死锁）: {}", src),
    }
}

/// as_ints 把 Array 值转为 i64 向量（便于断言）。
fn as_ints(v: Value) -> Vec<i64> {
    match v {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            g.iter()
                .map(|x| match x {
                    Value::Int(n) => *n,
                    other => panic!("expected Int, got {}", other.type_name()),
                })
                .collect()
        }
        other => panic!("expected Array, got {}", other.type_name()),
    }
}

// ---- chanRecv 并发接收（修复：去掉 rx 的 Mutex 包装） ----

#[test]
fn test_chanrecv_concurrent_same_channel() {
    // 3 个线程同时对同一 channel 调用 chanRecv，各取到一条数据。
    // 修复前 rx 被 Mutex 包裹：阻塞接收期间持有互斥锁，并发接收被串行化，
    // 且与 chanTryRecv 组合时可死锁。
    let src = r#"
var ch = newChannel()
var got = [0, 0, 0]
func r0() { got[0] = chanRecv(ch) ?? 1000 }
func r1() { got[1] = chanRecv(ch) ?? 2000 }
func r2() { got[2] = chanRecv(ch) ?? 3000 }
func producer() {
    chanSend(ch, 100)
    chanSend(ch, 200)
    chanSend(ch, 300)
}
run r0()
run r1()
run r2()
sleepMs(100)
run producer()
sleepMs(300)
return got
"#;
    let mut vals = as_ints(run_with_timeout(src, Duration::from_secs(15)));
    vals.sort();
    // 三个接收者各自恰好取到 100/200/300 中的一个（无丢失、无重复）
    assert_eq!(vals, vec![100, 200, 300], "3 个并发接收者应各取到一条数据");
}

#[test]
fn test_chanrecv_blocked_does_not_block_tryrecv() {
    // 一个线程阻塞在 chanRecv 上时，另一个线程的 chanTryRecv 必须立即返回
    // undefined（不被接收端的锁卡住）。修复前此场景会永久死锁。
    let src = r#"
var ch = newChannel()
var res = [0, 0]
func blocker() { res[0] = chanRecv(ch) }
run blocker()
sleepMs(150)  // 确保 blocker 已进入阻塞接收
var t0 = now()
var tv = chanTryRecv(ch)
var dt = now() - t0
chanSend(ch, 42)  // 唤醒 blocker
sleepMs(150)
return [res[0], tv == undefined ? 1 : 0, dt < 1000 ? 1 : 0]
"#;
    let r = run_with_timeout(src, Duration::from_secs(15));
    let vals = as_ints(r);
    assert_eq!(vals[0], 42, "blocker 应收到 42");
    assert_eq!(vals[1], 1, "chanTryRecv 应立即返回 undefined");
    assert_eq!(vals[2], 1, "chanTryRecv 耗时应在 1 秒内（非阻塞）");
}

// ---- RWMutex 压力（修复：单一 Mutex + Condvar，消除 ABBA） ----

#[test]
fn test_rwmutex_stress_no_deadlock() {
    // 3 读者 + 2 写者高频竞争同一读写锁，WaitGroup 汇合。
    // 修复前 rlock/wlock 以相反顺序获取两把锁（ABBA），此场景可死锁；
    // 现固定单一互斥锁，且写者等待时新读者排队（防写者饥饿），写计数应精确。
    let src = r#"
var rw = newRWMutex()
var wg = newWaitGroup()
var counter = [0]
func reader() {
    var i = 0
    while i < 200 {
        rlock(rw)
        var v = counter[0]
        runlock(rw)
        i = i + 1
    }
    wgDone(wg)
}
func writer() {
    var i = 0
    while i < 100 {
        wlock(rw)
        counter[0] = counter[0] + 1
        wunlock(rw)
        i = i + 1
    }
    wgDone(wg)
}
wgAdd(wg, 5)
run reader()
run reader()
run reader()
run writer()
run writer()
wgWait(wg)
wlock(rw)
wunlock(rw)
return counter[0]
"#;
    let r = run_with_timeout(src, Duration::from_secs(20));
    assert_eq!(r, Value::Int(200), "2 个写者各 +100，读写锁下应精确为 200");
}

// ---- onceDo 错误传播与递归检测 ----

#[test]
fn test_oncedo_callback_error_propagates() {
    // 回调抛错（除零）时，首次调用与后续调用都应返回该错误，且回调只执行一次。
    let src = r#"
var once = newOnce()
var cnt = [0]
var e1 = 0
var e2 = 0
func bad() {
    cnt[0] = cnt[0] + 1
    return 10 / 0
}
try {
    onceDo(once, bad)
} catch (e) {
    e1 = 1
}
try {
    onceDo(once, bad)
} catch (e) {
    e2 = 1
}
return [cnt[0], e1, e2]
"#;
    let vals = as_ints(run(src).expect("script should complete"));
    assert_eq!(vals, vec![1, 1, 1], "回调应只执行 1 次且两次调用均返回错误");
}

#[test]
fn test_oncedo_callback_error_returned_as_err() {
    // 不捕获时 onceDo 的错误应直接成为 run_string 的 Err（错误不被吞掉）。
    let r = run("var o = newOnce(); func bad() { return 10 / 0 }; onceDo(o, bad); return 1");
    assert!(r.is_err(), "onceDo 回调抛错应向外返回 Err");
}

#[test]
fn test_oncedo_recursive_returns_error() {
    // 回调内递归调用同一 once：应返回错误提示，而不是永久阻塞。
    let src = r#"
var once = newOnce()
var err = 0
func rec() {
    onceDo(once, rec)
}
try {
    onceDo(once, rec)
} catch (e) {
    err = 1
}
return err
"#;
    let r = run_with_timeout(src, Duration::from_secs(15));
    assert_eq!(r, Value::Int(1), "递归调用同一 once 应返回错误（err = 1）");
}

// ---- wgAdd 溢出 / 变负 ----

#[test]
fn test_wgadd_overflow_returns_error() {
    // 计数加 n 溢出 i64 范围时返回 error（修复前 fetch_add 会 panic 或回绕）。
    let src = r#"
var wg = newWaitGroup()
var r1 = 0
var r2 = 0
wgAdd(wg, 9223372036854775807)
try {
    wgAdd(wg, 1)
} catch (e) {
    r1 = 1
}
var wg2 = newWaitGroup()
try {
    wgAdd(wg2, -1)
} catch (e) {
    r2 = 1
}
return [r1, r2]
"#;
    let vals = as_ints(run(src).expect("script should complete"));
    assert_eq!(vals, vec![1, 1], "溢出与变负的 wgAdd 均应返回 error");
}

// ---- newSemaphore 非法参数 ----

#[test]
fn test_new_semaphore_invalid_args_return_error() {
    // to_int 失败（字符串）或 <= 0（0 / 负数）均应返回 error，不再静默取默认值。
    let src = r#"
var e0 = 0
var eneg = 0
var estr = 0
try { newSemaphore(0) } catch (e) { e0 = 1 }
try { newSemaphore(-3) } catch (e) { eneg = 1 }
try { newSemaphore("notnum") } catch (e) { estr = 1 }
return [e0, eneg, estr]
"#;
    let vals = as_ints(run(src).expect("script should complete"));
    assert_eq!(vals, vec![1, 1, 1], "0 / 负数 / 非整数参数均应返回 error");
}

#[test]
fn test_new_semaphore_valid_still_works() {
    // 合法参数回归：正常获取/释放
    let src = "var s = newSemaphore(2); semAcquire(s); semRelease(s); return 1";
    assert_eq!(run(src).expect("valid semaphore should work"), Value::Int(1));
}

// ---- 正常路径回归 ----

#[test]
fn test_regression_mutex_counter_2000() {
    // mutex 保护下 2 线程各 +1000，应精确 2000（WaitGroup 汇合，不靠 sleep）。
    let src = r#"
var counter = [0]
var mu = newMutex()
var wg = newWaitGroup()
func worker() {
    var i = 0
    while i < 1000 {
        lock(mu)
        counter[0] = counter[0] + 1
        unlock(mu)
        i = i + 1
    }
    wgDone(wg)
}
wgAdd(wg, 2)
run worker()
run worker()
wgWait(wg)
return counter[0]
"#;
    let r = run_with_timeout(src, Duration::from_secs(20));
    assert_eq!(r, Value::Int(2000), "mutex 保护下 counter 应精确为 2000");
}

#[test]
fn test_regression_channel_producer_consumer() {
    // channel 生产消费回归：主线程消费 10 条数据，无丢失。
    let src = r#"
var ch = newChannel()
var results = []
func producer() {
    var i = 1
    while i <= 10 {
        chanSend(ch, i)
        i = i + 1
    }
    chanSend(ch, -1)
}
run producer()
while true {
    var v = chanRecv(ch)
    if v == -1 { break }
    push(results, v)
}
return len(results)
"#;
    let r = run_with_timeout(src, Duration::from_secs(15));
    assert_eq!(r, Value::Int(10), "channel 生产消费应收到 10 个值");
}

#[test]
fn test_regression_rwmutex_basic_ops() {
    // 基本读写锁语义回归：多读可共存、写独占。
    let src = r#"
var rw = newRWMutex()
rlock(rw)
rlock(rw)
rlock(rw)
runlock(rw)
runlock(rw)
runlock(rw)
wlock(rw)
wunlock(rw)
return 1
"#;
    let r = run_with_timeout(src, Duration::from_secs(10));
    assert_eq!(r, Value::Int(1), "rwmutex 基本操作应正常");
}
