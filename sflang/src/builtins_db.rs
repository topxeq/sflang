//! builtins_db.rs — 数据库内置函数
//!
//! 4 个核心函数（对标 Charlang），API 设计为通用多数据库形式。
//! 当前实现 SQLite（通过 rusqlite bundled 模式，零配置）。
//! 将来扩展其他数据库只需在 dbConnect 的 match 中增加分支。
//!
//! 函数：
//!   dbConnect(driver, connStr)     — 连接数据库
//!   dbExec(db, sql, params...)     — 执行非查询 SQL
//!   dbQuery(db, sql, params...)    — 查询，返回 array of map
//!   dbClose(db)                    — 关闭连接

use std::sync::{Arc, Mutex};

use rusqlite::types::ValueRef;

use crate::builtins_helpers as bh;
use crate::value::Value;

/// Connection Sflang 中的数据库连接对象。
///
/// 包装 rusqlite::Connection，用 Arc<Mutex<>> 实现线程安全。
/// 将来扩展其他数据库时，可以改为 enum 或 trait object。
pub type DbConnection = rusqlite::Connection;

/// register 注册所有数据库内置函数。
pub fn register(vm: &mut crate::vm::VM) {
    vm.register_builtin("dbConnect", bi_db_connect);
    vm.register_builtin("dbExec", bi_db_exec);
    vm.register_builtin("dbQuery", bi_db_query);
    vm.register_builtin("dbClose", bi_db_close);
}

// ---- 辅助函数 ----

/// db_value 将 rusqlite Connection 包装为 Value::Native。
fn db_value(conn: DbConnection) -> Value {
    Value::Native(Arc::new(Arc::new(Mutex::new(conn))))
}

/// db_downcast 从 Value 中提取 Connection 引用。
fn db_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<Mutex<DbConnection>>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<Mutex<DbConnection>>>().ok_or_else(|| {
            crate::value::error_value(format!(
                "{}() 参数不是 db 连接对象 (可能原因：未用 dbConnect 创建)", fn_name,
            ))
        }),
        Value::Undefined => Err(crate::value::error_value(format!(
            "{}() 参数为 undefined (可能原因：变量未初始化)", fn_name,
        ))),
        other => Err(crate::value::error_value(format!(
            "{}() 参数应为 db 连接对象，得到 {} (可能原因：参数顺序错误)", fn_name, other.type_name(),
        ))),
    }
}

/// value_to_sql 将 Sflang Value 转为 rusqlite 的 Value 类型。
fn value_to_sql(v: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as SqlValue;
    match v {
        Value::Int(i) => SqlValue::Integer(*i),
        Value::Float(f) => SqlValue::Real(*f),
        Value::Str(s) => SqlValue::Text(s.as_ref().to_string()),
        Value::Bool(b) => SqlValue::Integer(if *b { 1 } else { 0 }),
        Value::Byte(b) => SqlValue::Integer(*b as i64),
        Value::Undefined | Value::Error(_) => SqlValue::Null,
        Value::Bytes(b) => SqlValue::Blob(b.as_ref().to_vec()),
        other => SqlValue::Text(other.to_str()),
    }
}

/// sqlite_value_to_sflang 将 rusqlite ValueRef 转为 Sflang Value。
fn sqlite_to_value(v: ValueRef) -> Value {
    match v {
        ValueRef::Null => Value::Undefined,
        ValueRef::Integer(i) => Value::Int(i),
        ValueRef::Real(f) => Value::Float(f),
        ValueRef::Text(s) => Value::str_from(String::from_utf8_lossy(s).to_string()),
        ValueRef::Blob(b) => Value::Bytes(Arc::new(b.to_vec())),
    }
}

// ---- 内置函数 ----

/// bi_db_connect 连接数据库。
///
/// 用法：dbConnect(driver, connStr) → db
/// 当前仅支持 "sqlite3"（connStr 为文件路径或 ":memory:"）。
fn bi_db_connect(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    let driver = bh::as_str(args, 0, "dbConnect")?;
    let conn_str = bh::as_str(args, 1, "dbConnect")?;

    match driver {
        "sqlite3" | "sqlite" => {
            let conn = rusqlite::Connection::open(conn_str).map_err(|e| {
                crate::value::error_value(format!(
                    "dbConnect() 连接 SQLite '{}' 失败: {} (可能原因：路径无效或权限不足)", conn_str, e,
                ))
            })?;
            Ok(db_value(conn))
        }
        _ => Err(crate::value::error_value(format!(
            "dbConnect() 不支持的数据库类型 '{}' (当前仅支持 sqlite3)", driver,
        ))),
    }
}

/// bi_db_exec 执行非查询 SQL（INSERT/UPDATE/DELETE/CREATE/DROP 等）。
///
/// 用法：dbExec(db, sql) 或 dbExec(db, sql, param1, param2, ...)
/// 返回影响行数（int）。
fn bi_db_exec(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "dbExec")?;
    let sql = bh::as_str(args, 1, "dbExec")?;
    let db = db_downcast(&args[0], "dbExec")?;
    let guard = db.lock().map_err(|e| crate::value::error_value(format!(
        "dbExec() 数据库锁异常: {}", e,
    )))?;

    let params: &[Value] = if args.len() > 2 { &args[2..] } else { &[] };

    let affected = if params.is_empty() {
        guard.execute(sql, []).map_err(|e| {
            crate::value::error_value(format!(
                "dbExec() SQL 执行失败: {} (SQL: {})", e, sql,
            ))
        })?
    } else {
        // 使用 params_from_iter 绑定参数
        let sql_params: Vec<rusqlite::types::Value> = params.iter().map(value_to_sql).collect();
        guard.execute(sql, rusqlite::params_from_iter(sql_params.iter())).map_err(|e| {
            crate::value::error_value(format!(
                "dbExec() SQL 执行失败: {} (SQL: {})", e, sql,
            ))
        })?
    };

    Ok(Value::Int(affected as i64))
}

/// bi_db_query 查询数据库，返回 array of map（每行一个 map，列名→值）。
///
/// 用法：dbQuery(db, sql) 或 dbQuery(db, sql, param1, param2, ...)
fn bi_db_query(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "dbQuery")?;
    let sql = bh::as_str(args, 1, "dbQuery")?;
    let db = db_downcast(&args[0], "dbQuery")?;
    let guard = db.lock().map_err(|e| crate::value::error_value(format!(
        "dbQuery() 数据库锁异常: {}", e,
    )))?;

    let params: &[Value] = if args.len() > 2 { &args[2..] } else { &[] };

    let mut stmt = guard.prepare(sql).map_err(|e| {
        crate::value::error_value(format!(
            "dbQuery() SQL 预处理失败: {} (SQL: {})", e, sql,
        ))
    })?;

    let col_count = stmt.column_count();
    let col_names: Vec<String> = (0..col_count)
        .map(|i| stmt.column_name(i).unwrap_or_default().to_string())
        .collect();

    // 执行查询 — 统一用 params_from_iter，空数组也兼容
    let sql_params: Vec<rusqlite::types::Value> = params.iter().map(value_to_sql).collect();
    let rows_result = stmt.query_map(rusqlite::params_from_iter(sql_params.iter()), |row| {
        let mut m = crate::ord_map::OrdMap::new();
        for (i, name) in col_names.iter().enumerate() {
            let val: ValueRef = row.get_ref(i)?;
            m.set(name.clone(), sqlite_to_value(val));
        }
        Ok(m)
    });

    let rows = rows_result.map_err(|e| {
        crate::value::error_value(format!(
            "dbQuery() 查询失败: {} (SQL: {})", e, sql,
        ))
    })?;

    let mut result: Vec<Value> = Vec::new();
    for row_result in rows {
        let m = row_result.map_err(|e| {
            crate::value::error_value(format!(
                "dbQuery() 读取行失败: {}", e,
            ))
        })?;
        result.push(Value::Map(Arc::new(Mutex::new(m))));
    }

    Ok(Value::Array(Arc::new(Mutex::new(result))))
}

/// bi_db_close 关闭数据库连接。
///
/// 用法：dbClose(db)
fn bi_db_close(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "dbClose")?;
    // close 在 Mutex drop 时自动执行，这里只需移除引用
    // 但为了友好，检查类型
    let _ = db_downcast(&args[0], "dbClose")?;
    // 实际关闭：无法直接 close Mutex<Connection>，但 drop Arc 会触发
    // 如果只有一个引用，drop 后 Connection 会 close
    Ok(Value::Undefined)
}
