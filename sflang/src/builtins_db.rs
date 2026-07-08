//! builtins_db.rs — 数据库内置函数（多数据库后端）
//!
//! 4 个核心函数（对标 Charlang），API 设计为通用多数据库形式。
//! 通过 DatabaseConn 枚举支持多种数据库后端。
//!
//! 当前支持：
//!   - sqlite3 / sqlite — rusqlite（bundled，零配置）
//!   - mysql — mysql crate（纯 Rust，连接池）
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

/// DatabaseConn 统一的数据库连接（支持多种数据库后端）。
///
/// 新增数据库时：在此枚举增加变体 + 在 dbConnect/dbExec/dbQuery/dbClose 中增加 match 分支。
pub enum DatabaseConn {
    /// SQLite 连接（rusqlite）。
    Sqlite(Mutex<rusqlite::Connection>),
    /// MySQL 连接池（mysql crate）。
    /// Pool 内部是 Arc 共享的，本身线程安全，无需额外 Mutex。
    Mysql(mysql::Pool),
}

/// register 注册所有数据库内置函数。
pub fn register(vm: &mut crate::vm::VM) {
    vm.register_builtin("dbConnect", bi_db_connect);
    vm.register_builtin("dbExec", bi_db_exec);
    vm.register_builtin("dbQuery", bi_db_query);
    vm.register_builtin("dbClose", bi_db_close);
}

// ---- 辅助：包装与提取 ----

/// db_value 将 DatabaseConn 包装为 Value::Native。
fn db_value(conn: DatabaseConn) -> Value {
    Value::Native(Arc::new(conn))
}

/// db_downcast 从 Value 中提取 DatabaseConn 引用。
fn db_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a DatabaseConn, Value> {
    match v {
        Value::Native(n) => {
            n.downcast_ref::<DatabaseConn>().ok_or_else(|| {
                crate::value::error_value(format!(
                    "{}() 参数不是 db 连接对象 (可能原因：未用 dbConnect 创建)", fn_name,
                ))
            })
        }
        Value::Undefined => Err(crate::value::error_value(format!(
            "{}() 参数为 undefined (可能原因：变量未初始化)", fn_name,
        ))),
        other => Err(crate::value::error_value(format!(
            "{}() 参数应为 db 连接对象，得到 {} (可能原因：参数顺序错误)", fn_name, other.type_name(),
        ))),
    }
}

// ---- 辅助：SQLite 值转换 ----

/// value_to_sqlite 将 Sflang Value 转为 rusqlite 的 Value 类型。
fn value_to_sqlite(v: &Value) -> rusqlite::types::Value {
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

/// sqlite_to_value 将 rusqlite ValueRef 转为 Sflang Value。
fn sqlite_to_value(v: ValueRef) -> Value {
    match v {
        ValueRef::Null => Value::Undefined,
        ValueRef::Integer(i) => Value::Int(i),
        ValueRef::Real(f) => Value::Float(f),
        ValueRef::Text(s) => Value::str_from(String::from_utf8_lossy(s).to_string()),
        ValueRef::Blob(b) => Value::Bytes(Arc::new(b.to_vec())),
    }
}

// ---- 辅助：MySQL 值转换 ----

/// value_to_mysql 将 Sflang Value 转为 mysql::Value。
fn value_to_mysql(v: &Value) -> mysql::Value {
    use mysql::Value as Mv;
    match v {
        Value::Int(i) => Mv::Int(*i),
        Value::Float(f) => Mv::Float(*f as f32),
        Value::Str(s) => Mv::Bytes(s.as_bytes().to_vec()),
        Value::Bool(b) => Mv::Int(if *b { 1 } else { 0 }),
        Value::Byte(b) => Mv::UInt(*b as u64),
        Value::Undefined | Value::Error(_) => Mv::NULL,
        Value::Bytes(b) => Mv::Bytes(b.as_ref().clone()),
        other => Mv::Bytes(other.to_str().into_bytes()),
    }
}

/// mysql_to_value 将 mysql::Value 转为 Sflang Value。
fn mysql_to_value(v: &mysql::Value) -> Value {
    use mysql::Value as Mv;
    match v {
        Mv::NULL => Value::Undefined,
        Mv::Int(i) => Value::Int(*i),
        Mv::UInt(u) => Value::Int(*u as i64),
        Mv::Float(f) => Value::Float(*f as f64),
        Mv::Bytes(b) => Value::str_from(String::from_utf8_lossy(b).to_string()),
        // Date/Time 类型转字符串
        other => Value::str_from(format!("{}", other.as_sql(false))),
    }
}

// ---- 内置函数 ----

/// bi_db_connect 连接数据库。
///
/// 用法：dbConnect(driver, connStr) → db
///
/// driver:
///   "sqlite3" / "sqlite" — connStr 为文件路径或 ":memory:"
///   "mysql"              — connStr 为 mysql://user:pass@host:port/db
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
            Ok(db_value(DatabaseConn::Sqlite(Mutex::new(conn))))
        }
        "mysql" => {
            let opts = mysql::Opts::from_url(conn_str).map_err(|e| {
                crate::value::error_value(format!(
                    "dbConnect() MySQL 连接字符串解析失败: {} (格式: mysql://user:pass@host:port/db)", e,
                ))
            })?;
            let pool = mysql::Pool::new(opts).map_err(|e| {
                crate::value::error_value(format!(
                    "dbConnect() 连接 MySQL 失败: {} (可能原因：网络不通、认证失败、数据库不存在)", e,
                ))
            })?;
            Ok(db_value(DatabaseConn::Mysql(pool)))
        }
        _ => Err(crate::value::error_value(format!(
            "dbConnect() 不支持的数据库类型 '{}' (当前支持: sqlite3, mysql)", driver,
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
    let params: &[Value] = if args.len() > 2 { &args[2..] } else { &[] };

    match db {
        DatabaseConn::Sqlite(conn) => {
            let guard = conn.lock().map_err(|e| crate::value::error_value(format!(
                "dbExec() 数据库锁异常: {}", e,
            )))?;
            let affected = if params.is_empty() {
                guard.execute(sql, []).map_err(|e| {
                    crate::value::error_value(format!("dbExec() SQL 执行失败: {} (SQL: {})", e, sql))
                })?
            } else {
                let sql_params: Vec<rusqlite::types::Value> = params.iter().map(value_to_sqlite).collect();
                guard.execute(sql, rusqlite::params_from_iter(sql_params.iter())).map_err(|e| {
                    crate::value::error_value(format!("dbExec() SQL 执行失败: {} (SQL: {})", e, sql))
                })?
            };
            Ok(Value::Int(affected as i64))
        }
        DatabaseConn::Mysql(pool) => {
            use mysql::prelude::*;
            let mut conn = pool.get_conn().map_err(|e| {
                crate::value::error_value(format!("dbExec() 获取 MySQL 连接失败: {}", e))
            })?;
            let mysql_params: Vec<mysql::Value> = params.iter().map(value_to_mysql).collect();
            if mysql_params.is_empty() {
                conn.query_drop(sql).map_err(|e| {
                    crate::value::error_value(format!("dbExec() SQL 执行失败: {} (SQL: {})", e, sql))
                })?;
                Ok(Value::Int(conn.affected_rows() as i64))
            } else {
                conn.exec_drop(sql, mysql_params).map_err(|e| {
                    crate::value::error_value(format!("dbExec() SQL 执行失败: {} (SQL: {})", e, sql))
                })?;
                Ok(Value::Int(conn.affected_rows() as i64))
            }
        }
    }
}

/// bi_db_query 查询数据库，返回 array of map（每行一个 map，列名→值）。
///
/// 用法：dbQuery(db, sql) 或 dbQuery(db, sql, param1, param2, ...)
fn bi_db_query(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "dbQuery")?;
    let sql = bh::as_str(args, 1, "dbQuery")?;
    let db = db_downcast(&args[0], "dbQuery")?;
    let params: &[Value] = if args.len() > 2 { &args[2..] } else { &[] };

    match db {
        DatabaseConn::Sqlite(conn) => {
            let guard = conn.lock().map_err(|e| crate::value::error_value(format!(
                "dbQuery() 数据库锁异常: {}", e,
            )))?;
            let mut stmt = guard.prepare(sql).map_err(|e| {
                crate::value::error_value(format!("dbQuery() SQL 预处理失败: {} (SQL: {})", e, sql))
            })?;
            let col_count = stmt.column_count();
            let col_names: Vec<String> = (0..col_count)
                .map(|i| stmt.column_name(i).unwrap_or_default().to_string())
                .collect();
            let sql_params: Vec<rusqlite::types::Value> = params.iter().map(value_to_sqlite).collect();
            let rows = stmt.query_map(rusqlite::params_from_iter(sql_params.iter()), |row| {
                let mut m = crate::ord_map::OrdMap::new();
                for (i, name) in col_names.iter().enumerate() {
                    let val: ValueRef = row.get_ref(i)?;
                    m.set(name.clone(), sqlite_to_value(val));
                }
                Ok(m)
            }).map_err(|e| {
                crate::value::error_value(format!("dbQuery() 查询失败: {} (SQL: {})", e, sql))
            })?;

            let mut result: Vec<Value> = Vec::new();
            for row_result in rows {
                let m = row_result.map_err(|e| {
                    crate::value::error_value(format!("dbQuery() 读取行失败: {}", e))
                })?;
                result.push(Value::Map(Arc::new(Mutex::new(m))));
            }
            Ok(Value::Array(Arc::new(Mutex::new(result))))
        }
        DatabaseConn::Mysql(pool) => {
            use mysql::prelude::*;
            let mut conn = pool.get_conn().map_err(|e| {
                crate::value::error_value(format!("dbQuery() 获取 MySQL 连接失败: {}", e))
            })?;

            // MySQL 查询：统一用 exec_iter（空参数传空 vec）
            let mysql_params: Vec<mysql::Value> = params.iter().map(value_to_mysql).collect();

            let result_set = conn.exec_iter(sql, mysql_params);

            let mut result_set = result_set.map_err(|e| {
                crate::value::error_value(format!("dbQuery() 查询失败: {} (SQL: {})", e, sql))
            })?;

            // 获取列名（columns() 返回 SetColumns，实现 AsRef<[Column]>）
            let cols = result_set.columns();
            let col_names: Vec<String> = cols.as_ref().iter().map(|c| c.name_str().to_string()).collect();

            let mut result: Vec<Value> = Vec::new();
            for row_result in result_set.by_ref() {
                let row = row_result.map_err(|e| {
                    crate::value::error_value(format!("dbQuery() 读取行失败: {}", e))
                })?;
                let mut m = crate::ord_map::OrdMap::new();
                let values = row.unwrap();
                for (i, name) in col_names.iter().enumerate() {
                    let val = values.get(i).unwrap_or(&mysql::Value::NULL);
                    m.set(name.clone(), mysql_to_value(val));
                }
                result.push(Value::Map(Arc::new(Mutex::new(m))));
            }
            Ok(Value::Array(Arc::new(Mutex::new(result))))
        }
    }
}

/// bi_db_close 关闭数据库连接。
///
/// 用法：dbClose(db)
fn bi_db_close(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "dbClose")?;
    let db = db_downcast(&args[0], "dbClose")?;
    // SQLite: Connection 在 Mutex drop 时自动关闭
    // MySQL: Pool 在 Arc drop 时自动关闭连接
    // 此处只需确保引用有效即可，实际关闭由 Arc 的引用计数管理
    match db {
        DatabaseConn::Sqlite(_) => {}
        DatabaseConn::Mysql(_) => {}
    }
    Ok(Value::Undefined)
}
