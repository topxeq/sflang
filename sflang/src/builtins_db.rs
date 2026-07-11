//! builtins_db.rs — 数据库内置函数（多数据库后端）
//!
//! 4 个核心函数（对标 Charlang），API 设计为通用多数据库形式。
//! 通过 DatabaseConn 枚举支持多种数据库后端。
//!
//! 当前支持：
//!   - sqlite3 / sqlite — rusqlite（bundled，零配置）
//!   - mysql — mysql crate（纯 Rust，连接池）
//!   - postgres — postgres crate（同步 API）
//!   - mssql / sqlserver — tiberius crate（纯 Rust TDS，tokio 桥接同步）
//!   - oracle — oracle-rs crate（纯 Rust TNS，tokio 桥接同步）
//!
//! 函数：
//!   dbConnect(driver, connStr)      — 连接数据库
//!   dbExec(db, sql, params...)      — 执行非查询 SQL
//!   dbQuery(db, sql, params...)     — 查询，返回 array of map（列名→值）
//!   dbQueryRecs(db, sql, params...) — 查询，返回二维数组（首行列名+数据行）
//!   dbQueryCount(db, sql, params..) — 查询单值→int（如 COUNT）
//!   dbQueryFloat(db, sql, params..) — 查询单值→float（如 AVG/SUM）
//!   dbQueryString(db, sql, params..)— 查询单值→string（如取名称）
//!   dbQueryMap(db, keyCol, sql...)  — 按指定列索引→map（一对一）
//!   dbQueryMapArray(db, keyCol, ...)— 按指定列分组→map（一对多）
//!   dbClose(db)                     — 关闭连接

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
    /// PostgreSQL 连接（postgres crate，同步 Client）。
    Postgres(Mutex<postgres::Client>),
    /// MSSQL 连接（tiberius crate，异步 Client + tokio runtime 桥接同步）。
    /// Client 的泛型参数是 compat 后的 TcpStream（futures AsyncRead/Write）。
    Mssql(Mutex<tiberius::Client<tokio_util::compat::Compat<tokio::net::TcpStream>>>, tokio::runtime::Runtime),
    /// Oracle 连接（oracle-rs crate，异步 Connection + tokio runtime 桥接同步）。
    Oracle(Mutex<oracle_rs::Connection>, tokio::runtime::Runtime),
}

/// parse_mysql_conn_str 手动解析 MySQL 连接字符串。
///
/// 当 mysql::Opts::from_url 因密码含特殊字符（如 #、@、/ 等）而失败时使用。
/// 格式：mysql://user:pass@host:port/db
///
/// 返回 None 表示格式完全无法识别。
fn parse_mysql_conn_str(s: &str) -> Option<mysql::OptsBuilder> {
    // 去掉 mysql:// 前缀
    let rest = s.strip_prefix("mysql://")?;

    // 找最后一个 @（密码中可能含 @，取最后一个作为分隔）
    let at_pos = rest.rfind('@')?;
    let user_pass = &rest[..at_pos];
    let host_db = &rest[at_pos + 1..];

    // 解析 user:pass
    let (user, pass) = match user_pass.find(':') {
        Some(pos) => (&user_pass[..pos], &user_pass[pos + 1..]),
        None => (user_pass, ""),
    };

    // 解析 host:port/db
    // 先分离 db（最后一个 / 之后）
    let (host_port, db) = match host_db.rfind('/') {
        Some(pos) => (&host_db[..pos], &host_db[pos + 1..]),
        None => (host_db, ""),
    };

    // 解析 host:port
    let (host, port) = match host_port.rfind(':') {
        Some(pos) => {
            let p: u16 = host_port[pos + 1..].parse().ok()?;
            (&host_port[..pos], p)
        }
        None => (host_port, 3306u16),
    };

    let mut builder = mysql::OptsBuilder::default();
    builder = builder.ip_or_hostname(Some(host.to_string()))
        .tcp_port(port)
        .user(Some(user.to_string()))
        .pass(Some(pass.to_string()));  // 原始密码，不做 URL 解码
    if !db.is_empty() {
        builder = builder.db_name(Some(db.to_string()));
    }

    Some(builder)
}

/// register 注册所有数据库内置函数。
pub fn register(vm: &mut crate::vm::VM) {
    vm.register_builtin("dbConnect", bi_db_connect);
    vm.register_builtin("dbExec", bi_db_exec);
    vm.register_builtin("dbQuery", bi_db_query);
    vm.register_builtin("dbQueryRecs", bi_db_query_recs);
    vm.register_builtin("dbQueryCount", bi_db_query_count);
    vm.register_builtin("dbQueryFloat", bi_db_query_float);
    vm.register_builtin("dbQueryString", bi_db_query_string);
    vm.register_builtin("dbQueryStr", bi_db_query_string);  // Charlang 兼容别名
    vm.register_builtin("dbQueryMap", bi_db_query_map);
    vm.register_builtin("dbQueryMapArray", bi_db_query_map_array);
    vm.register_builtin("dbQueryOrdered", bi_db_query_ordered);
    vm.register_builtin("dbClose", bi_db_close);
    vm.register_builtin("formatSqlValue", bi_format_sql_value);
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

// ---- 辅助：PostgreSQL 值转换 ----

/// value_to_pg 将 Sflang Value 转为 postgres 能接受的 Box<dyn ToSql + Sync>。
fn value_to_pg_type(v: &Value) -> Box<dyn postgres_types::ToSql + Sync> {
    match v {
        Value::Int(i) => Box::new(*i as i32),  // PG 默认用 i32
        Value::Float(f) => Box::new(*f as f64),
        Value::Str(s) => Box::new(s.as_ref().to_string()),
        Value::Bool(b) => Box::new(*b),
        Value::Undefined | Value::Error(_) => Box::new(Option::<String>::None),
        other => Box::new(other.to_str()),
    }
}

/// pg_row_to_ordmap 将 postgres Row 转为 OrdMap。
fn pg_row_to_ordmap(row: &postgres::Row) -> crate::ord_map::OrdMap {
    let mut m = crate::ord_map::OrdMap::new();
    for (i, col) in row.columns().iter().enumerate() {
        let name = col.name().to_string();
        let val = pg_get_value(row, i, col.type_());
        m.set(name, val);
    }
    m
}

/// pg_get_value 从 postgres Row 中按类型安全地取值。
fn pg_get_value(row: &postgres::Row, idx: usize, ty: &postgres::types::Type) -> Value {
    use postgres::types::Type;
    match *ty {
        Type::INT2 => {
            let v: Option<i16> = row.try_get(idx).unwrap_or(None);
            match v { Some(i) => Value::Int(i as i64), None => Value::Undefined }
        }
        Type::INT4 => {
            let v: Option<i32> = row.try_get(idx).unwrap_or(None);
            match v { Some(i) => Value::Int(i as i64), None => Value::Undefined }
        }
        Type::INT8 => {
            let v: Option<i64> = row.try_get(idx).unwrap_or(None);
            match v { Some(i) => Value::Int(i), None => Value::Undefined }
        }
        Type::FLOAT4 => {
            let v: Option<f32> = row.try_get(idx).unwrap_or(None);
            match v { Some(f) => Value::Float(f as f64), None => Value::Undefined }
        }
        Type::FLOAT8 => {
            let v: Option<f64> = row.try_get(idx).unwrap_or(None);
            match v { Some(f) => Value::Float(f), None => Value::Undefined }
        }
        Type::BOOL => {
            let v: Option<bool> = row.try_get(idx).unwrap_or(None);
            match v { Some(b) => Value::Bool(b), None => Value::Undefined }
        }
        Type::TEXT | Type::VARCHAR | Type::NAME => {
            let v: Option<String> = row.try_get(idx).unwrap_or(None);
            match v { Some(s) => Value::str_from(s), None => Value::Undefined }
        }
        Type::BYTEA => {
            let v: Option<Vec<u8>> = row.try_get(idx).unwrap_or(None);
            match v { Some(b) => Value::Bytes(Arc::new(b)), None => Value::Undefined }
        }
        _ => {
            // 其他类型尝试作为字符串
            let v: Option<String> = row.try_get(idx).unwrap_or(None);
            match v { Some(s) => Value::str_from(s), None => Value::Undefined }
        }
    }
}

/// convert_placeholders 将 SQL 中的 ? 占位符转换为 PostgreSQL 的 $N 格式。
///
/// PostgreSQL 用 $1 $2 $3...，我们统一暴露 ? 给用户。
fn convert_pg_placeholders(sql: &str) -> String {
    let mut result = String::with_capacity(sql.len());
    let mut param_num = 0;
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'?' {
            param_num += 1;
            result.push_str(&format!("${}", param_num));
        } else {
            // 安全处理多字节字符
            let ch_len = if bytes[i] < 0x80 { 1 } else if bytes[i] < 0xC0 { 1 } else if bytes[i] < 0xE0 { 2 } else if bytes[i] < 0xF0 { 3 } else { 4 };
            let end = (i + ch_len).min(bytes.len());
            if let Ok(s) = std::str::from_utf8(&bytes[i..end]) {
                result.push_str(s);
            }
            i = end - 1; // 循环末尾会 +1
        }
        i += 1;
    }
    result
}

// ---- 辅助：MSSQL (tiberius) 值转换与连接 ----

/// parse_mssql_conn_str 解析 MSSQL 连接字符串。
///
/// 支持两种格式：
///   1. URL 格式: mssql://user:pass@host:port/database
///   2. key=value 格式: server=host,port=1433,user=sa,password=pass,database=db
///
/// 返回 (Config, host, port)。
fn parse_mssql_conn_str(s: &str) -> Result<(tiberius::Config, String, u16), String> {
    let mut config = tiberius::Config::new();
    let mut host = String::from("localhost");
    let mut port: u16 = 1433;

    if let Some(rest) = s.strip_prefix("mssql://").or_else(|| s.strip_prefix("sqlserver://")) {
        // URL 格式: mssql://user:pass@host:port/database
        let at_pos = rest.rfind('@').ok_or("缺少 @")?;
        let user_pass = &rest[..at_pos];
        let host_db = &rest[at_pos + 1..];

        let (user, pass) = match user_pass.find(':') {
            Some(p) => (&user_pass[..p], &user_pass[p + 1..]),
            None => (user_pass, ""),
        };

        let (host_port, db) = match host_db.rfind('/') {
            Some(p) => (&host_db[..p], &host_db[p + 1..]),
            None => (host_db, ""),
        };

        let (h, p) = match host_port.rfind(':') {
            Some(pos) => {
                let p: u16 = host_port[pos + 1..].parse().map_err(|_| "无效端口")?;
                (&host_port[..pos], p)
            }
            None => (host_port, 1433u16),
        };
        host = h.to_string();
        port = p;

        config.host(&host);
        config.port(port);
        config.authentication(tiberius::AuthMethod::sql_server(user, pass));
        if !db.is_empty() {
            config.database(db);
        }
    } else {
        // key=value 格式: server=host,port=1433,user=sa,password=pass,database=db
        let mut user = String::new();
        let mut pass = String::new();
        for part in s.split(|c| c == ',' || c == ';') {
            let part = part.trim();
            if let Some(eq) = part.find('=') {
                let key = part[..eq].trim().to_lowercase();
                let val = part[eq + 1..].trim();
                match key.as_str() {
                    "server" | "host" | "addr" => { host = val.to_string(); config.host(&host); }
                    "port" => {
                        if let Ok(p) = val.parse::<u16>() { port = p; config.port(port); }
                    }
                    "user" | "uid" | "username" => user = val.to_string(),
                    "password" | "pwd" => pass = val.to_string(),
                    "database" | "db" => config.database(val),
                    _ => {}
                }
            }
        }
        if !user.is_empty() {
            config.authentication(tiberius::AuthMethod::sql_server(&user, &pass));
        }
    }

    Ok((config, host, port))
}

/// mssql_get_row_value 从 tiberius Row 中按列索引取值。
///
/// 使用列的列名作为 key，按类型尝试取值。tiberius 的 try_get 接受列名或索引。
fn mssql_get_row_value(row: &tiberius::Row, col: &tiberius::Column) -> Value {
    use tiberius::ColumnType;
    let name = col.name();
    match col.column_type() {
        ColumnType::Bit | ColumnType::Bitn => {
            row.try_get::<bool, _>(name).unwrap_or(None)
                .map(Value::Bool).unwrap_or(Value::Undefined)
        }
        ColumnType::Int1 | ColumnType::Int2 | ColumnType::Int4 | ColumnType::Intn => {
            row.try_get::<i32, _>(name).unwrap_or(None)
                .map(|v| Value::Int(v as i64)).unwrap_or(Value::Undefined)
        }
        ColumnType::Int8 => {
            row.try_get::<i64, _>(name).unwrap_or(None)
                .map(Value::Int).unwrap_or(Value::Undefined)
        }
        ColumnType::Float4 | ColumnType::Floatn => {
            row.try_get::<f32, _>(name).unwrap_or(None)
                .map(|v| Value::Float(v as f64)).unwrap_or(Value::Undefined)
        }
        ColumnType::Float8 => {
            row.try_get::<f64, _>(name).unwrap_or(None)
                .map(Value::Float).unwrap_or(Value::Undefined)
        }
        ColumnType::NVarchar | ColumnType::NChar | ColumnType::BigVarChar | ColumnType::BigChar | ColumnType::Text | ColumnType::NText => {
            row.try_get::<&str, _>(name).unwrap_or(None)
                .map(Value::str).unwrap_or(Value::Undefined)
        }
        ColumnType::BigVarBin | ColumnType::BigBinary | ColumnType::Image => {
            row.try_get::<&[u8], _>(name).unwrap_or(None)
                .map(|v| Value::Bytes(Arc::new(v.to_vec()))).unwrap_or(Value::Undefined)
        }
        _ => {
            row.try_get::<&str, _>(name).unwrap_or(None)
                .map(Value::str).unwrap_or(Value::Undefined)
        }
    }
}

/// value_to_mssql 将 Sflang Value 转为 tiberius 能接受的参数。
fn value_to_mssql(v: &Value) -> Box<dyn tiberius::ToSql> {
    match v {
        Value::Int(i) => Box::new(*i),
        Value::Float(f) => Box::new(*f),
        Value::Str(s) => Box::new(s.as_ref().to_string()),
        Value::Bool(b) => Box::new(*b),
        Value::Undefined | Value::Error(_) => Box::new(Option::<i32>::None),
        other => Box::new(other.to_str()),
    }
}

// ---- 辅助：Oracle (oracle-rs) 值转换 ----

/// parse_oracle_conn_str 解析 Oracle 连接字符串。
///
/// 格式: oracle://user:pass@host:port/service
/// 或:   user/pass@host:port/service
fn parse_oracle_conn_str(s: &str) -> Result<(String, u16, String, String, String), String> {
    let rest = if let Some(r) = s.strip_prefix("oracle://") {
        r
    } else {
        s
    };

    let at_pos = rest.rfind('@').ok_or("缺少 @")?;
    let user_pass = &rest[..at_pos];
    let host_rest = &rest[at_pos + 1..];

    let (user, pass) = match user_pass.find(':') {
        Some(p) => (&user_pass[..p], &user_pass[p + 1..]),
        None => {
            // 也支持 user/pass 格式
            match user_pass.find('/') {
                Some(p) => (&user_pass[..p], &user_pass[p + 1..]),
                None => (user_pass, ""),
            }
        }
    };

    let (host_port, service) = match host_rest.rfind('/') {
        Some(p) => (&host_rest[..p], &host_rest[p + 1..]),
        None => (host_rest, ""),
    };

    let (host, port) = match host_port.rfind(':') {
        Some(p) => {
            let port: u16 = host_port[p + 1..].parse().map_err(|_| "无效端口")?;
            (&host_port[..p], port)
        }
        None => (host_port, 1521u16),
    };

    if service.is_empty() {
        return Err("缺少服务名/SID".to_string());
    }

    Ok((host.to_string(), port, service.to_string(), user.to_string(), pass.to_string()))
}

/// oracle_value_to_sflang 将 oracle-rs Value 转为 Sflang Value。
fn oracle_to_value(v: &oracle_rs::row::Value) -> crate::value::Value {
    use oracle_rs::row::Value as OraVal;
    use crate::value::Value as SfVal;
    match v {
        OraVal::Null => SfVal::Undefined,
        OraVal::Integer(i) => SfVal::Int(*i),
        OraVal::Float(f) => SfVal::Float(*f),
        OraVal::String(s) => SfVal::str(s),
        OraVal::Boolean(b) => SfVal::Bool(*b),
        OraVal::Bytes(b) => SfVal::Bytes(Arc::new(b.clone())),
        OraVal::Number(n) => SfVal::str_from(format!("{:?}", n)),
        OraVal::Date(_) => SfVal::Undefined,  // 简化处理
        OraVal::Timestamp(_) => SfVal::Undefined,
        _ => SfVal::Undefined,
    }
}

/// sflang_value_to_oracle 将 Sflang Value 转为 oracle-rs Value。
fn sflang_to_oracle(v: &crate::value::Value) -> oracle_rs::row::Value {
    use oracle_rs::row::Value as OraVal;
    use crate::value::Value as SfVal;
    match v {
        SfVal::Int(i) => OraVal::Integer(*i),
        SfVal::Float(f) => OraVal::Float(*f),
        SfVal::Str(s) => OraVal::String(s.as_ref().to_string()),
        SfVal::Bool(b) => OraVal::Boolean(*b),
        SfVal::Undefined | SfVal::Error(_) => OraVal::Null,
        SfVal::Bytes(b) => OraVal::Bytes(b.as_ref().clone()),
        other => OraVal::String(other.to_str()),
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

    // dbConnect 的错误返回错误对象（不抛异常），符合 Sflang "一般返回错误对象为主" 的原则
    match driver {
        "sqlite3" | "sqlite" => {
            match rusqlite::Connection::open(conn_str) {
                Ok(conn) => Ok(db_value(DatabaseConn::Sqlite(Mutex::new(conn)))),
                Err(e) => Ok(crate::value::error_value(format!(
                    "dbConnect() 连接 SQLite '{}' 失败: {} (可能原因：路径无效或权限不足)", conn_str, e,
                ))),
            }
        }
        "mysql" => {
            // 先尝试 URL 解析，失败则用 OptsBuilder 手动解析（支持密码含特殊字符）
            let opts = match mysql::Opts::from_url(conn_str) {
                Ok(o) => o,
                Err(_) => {
                    // URL 解析失败，用手动解析
                    match parse_mysql_conn_str(conn_str) {
                        Some(builder) => mysql::Opts::from(builder),
                        None => return Ok(crate::value::error_value(format!(
                            "dbConnect() MySQL 连接字符串解析失败: '{}' (格式: mysql://user:pass@host:port/db)", conn_str,
                        ))),
                    }
                }
            };
            match mysql::Pool::new(opts) {
                Ok(pool) => Ok(db_value(DatabaseConn::Mysql(pool))),
                Err(e) => Ok(crate::value::error_value(format!(
                    "dbConnect() 连接 MySQL 失败: {} (可能原因：网络不通、认证失败、数据库不存在)", e,
                ))),
            }
        }
        "postgres" | "postgresql" | "pg" => {
            use postgres::NoTls;
            match postgres::Client::connect(conn_str, NoTls) {
                Ok(client) => Ok(db_value(DatabaseConn::Postgres(Mutex::new(client)))),
                Err(e) => Ok(crate::value::error_value(format!(
                    "dbConnect() 连接 PostgreSQL 失败: {} (可能原因：网络不通、认证失败、数据库不存在)", e,
                ))),
            }
        }
        "mssql" | "sqlserver" | "mssqlserver" => {
            use tokio_util::compat::TokioAsyncReadCompatExt;
            let (config, host, port) = match parse_mssql_conn_str(conn_str) {
                Ok(v) => v,
                Err(e) => return Ok(crate::value::error_value(format!(
                    "dbConnect() MSSQL 连接字符串解析失败: {} (格式: mssql://user:pass@host:port/db)", e,
                ))),
            };
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => return Ok(crate::value::error_value(format!(
                    "dbConnect() 创建 tokio runtime 失败: {}", e,
                ))),
            };
            let result = runtime.block_on(async move {
                let tcp = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
                tcp.set_nodelay(true).ok();
                tiberius::Client::connect(config, tcp.compat()).await
            });
            match result {
                Ok(client) => Ok(db_value(DatabaseConn::Mssql(Mutex::new(client), runtime))),
                Err(e) => Ok(crate::value::error_value(format!(
                    "dbConnect() 连接 MSSQL 失败: {} (可能原因：网络不通、认证失败、数据库不存在)", e,
                ))),
            }
        }
        "oracle" => {
            let (host, port, service, user, pass) = match parse_oracle_conn_str(conn_str) {
                Ok(v) => v,
                Err(e) => return Ok(crate::value::error_value(format!(
                    "dbConnect() Oracle 连接字符串解析失败: {} (格式: oracle://user:pass@host:port/service)", e,
                ))),
            };
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => return Ok(crate::value::error_value(format!(
                    "dbConnect() 创建 tokio runtime 失败: {}", e,
                ))),
            };
            let config = oracle_rs::Config::new(&host, port, &service, &user, &pass);
            let result = runtime.block_on(async {
                oracle_rs::Connection::connect_with_config(config).await
            });
            match result {
                Ok(conn) => Ok(db_value(DatabaseConn::Oracle(Mutex::new(conn), runtime))),
                Err(e) => Ok(crate::value::error_value(format!(
                    "dbConnect() 连接 Oracle 失败: {} (可能原因：网络不通、认证失败、服务名错误)", e,
                ))),
            }
        }
        _ => Ok(crate::value::error_value(format!(
            "dbConnect() 不支持的数据库类型 '{}' (当前支持: sqlite3, mysql, postgres, mssql, oracle)", driver,
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
        DatabaseConn::Postgres(client) => {
            let mut guard = client.lock().map_err(|e| crate::value::error_value(format!(
                "dbExec() 数据库锁异常: {}", e,
            )))?;
            let pg_sql = convert_pg_placeholders(sql);
            // 构建 &dyn ToSql 参数
            let pg_params: Vec<Box<dyn postgres::types::ToSql + Sync>> =
                params.iter().map(value_to_pg_type).collect();
            let pg_refs: Vec<&(dyn postgres::types::ToSql + Sync)> =
                pg_params.iter().map(|b| b.as_ref()).collect();
            let result = guard.execute(&pg_sql, &pg_refs[..]);
            match result {
                Ok(n) => Ok(Value::Int(n as i64)),
                Err(e) => Ok(crate::value::error_value(format!(
                    "dbExec() SQL 执行失败: {} (SQL: {})", e, sql,
                ))),
            }
        }
        DatabaseConn::Mssql(client, runtime) => {
            let mut guard = client.lock().map_err(|e| crate::value::error_value(format!(
                "dbExec() 数据库锁异常: {}", e,
            )))?;
            let mssql_params: Vec<Box<dyn tiberius::ToSql>> = params.iter().map(value_to_mssql).collect();
            let param_refs: Vec<&dyn tiberius::ToSql> = mssql_params.iter().map(|b| b.as_ref()).collect();
            let result = runtime.block_on(async {
                if param_refs.is_empty() {
                    guard.simple_query(sql).await.map(|_| 0i64)
                } else {
                    guard.execute(sql, &param_refs).await.map(|r| r.total() as i64)
                }
            });
            match result {
                Ok(n) => Ok(Value::Int(n)),
                Err(e) => Ok(crate::value::error_value(format!(
                    "dbExec() SQL 执行失败: {} (SQL: {})", e, sql,
                ))),
            }
        }
        DatabaseConn::Oracle(conn, runtime) => {
            let guard = conn.lock().map_err(|e| crate::value::error_value(format!(
                "dbExec() 数据库锁异常: {}", e,
            )))?;
            let ora_params: Vec<oracle_rs::row::Value> = params.iter().map(sflang_to_oracle).collect();
            let result = runtime.block_on(async {
                guard.execute_dml_sql(sql, &ora_params).await
            });
            match result {
                Ok(n) => Ok(Value::Int(n as i64)),
                Err(e) => Ok(crate::value::error_value(format!(
                    "dbExec() SQL 执行失败: {} (SQL: {})", e, sql,
                ))),
            }
        }
    }
}
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
        DatabaseConn::Postgres(client) => {
            let mut guard = client.lock().map_err(|e| crate::value::error_value(format!(
                "dbQuery() 数据库锁异常: {}", e,
            )))?;
            let pg_sql = convert_pg_placeholders(sql);
            let pg_params: Vec<Box<dyn postgres::types::ToSql + Sync>> =
                params.iter().map(value_to_pg_type).collect();
            let pg_refs: Vec<&(dyn postgres::types::ToSql + Sync)> =
                pg_params.iter().map(|b| b.as_ref()).collect();

            let rows = guard.query(&pg_sql, &pg_refs[..]).map_err(|e| {
                crate::value::error_value(format!("dbQuery() 查询失败: {} (SQL: {})", e, sql))
            })?;

            let mut result: Vec<Value> = Vec::new();
            for row in &rows {
                result.push(Value::Map(Arc::new(Mutex::new(pg_row_to_ordmap(row)))));
            }
            Ok(Value::Array(Arc::new(Mutex::new(result))))
        }
        DatabaseConn::Mssql(client, runtime) => {
            let mut guard = client.lock().map_err(|e| crate::value::error_value(format!(
                "dbQuery() 数据库锁异常: {}", e,
            )))?;
            let mssql_params: Vec<Box<dyn tiberius::ToSql>> = params.iter().map(value_to_mssql).collect();
            let param_refs: Vec<&dyn tiberius::ToSql> = mssql_params.iter().map(|b| b.as_ref()).collect();

            let result = runtime.block_on(async {
                let stream = if param_refs.is_empty() {
                    guard.simple_query(sql).await
                } else {
                    guard.query(sql, &param_refs).await
                };
                match stream {
                    Ok(s) => {
                        use futures_util::TryStreamExt;
                        use tiberius::QueryItem;
                        let items: Vec<QueryItem> = s.try_collect().await?;
                        let rows: Vec<tiberius::Row> = items.into_iter().filter_map(|item| {
                            if let QueryItem::Row(row) = item { Some(row) } else { None }
                        }).collect();
                        Ok(rows)
                    }
                    Err(e) => Err(e),
                }
            });

            match result {
                Ok(rows) => {
                    let mut result: Vec<Value> = Vec::new();
                    for row in &rows {
                        let mut m = crate::ord_map::OrdMap::new();
                        for col in row.columns() {
                            let name = col.name().to_string();
                            let val = mssql_get_row_value(row, col);
                            m.set(name, val);
                        }
                        result.push(Value::Map(Arc::new(Mutex::new(m))));
                    }
                    Ok(Value::Array(Arc::new(Mutex::new(result))))
                }
                Err(e) => Ok(crate::value::error_value(format!(
                    "dbQuery() 查询失败: {} (SQL: {})", e, sql,
                ))),
            }
        }
        DatabaseConn::Oracle(conn, runtime) => {
            let guard = conn.lock().map_err(|e| crate::value::error_value(format!(
                "dbQuery() 数据库锁异常: {}", e,
            )))?;
            let ora_params: Vec<oracle_rs::row::Value> = params.iter().map(sflang_to_oracle).collect();
            let result = runtime.block_on(async {
                guard.query(sql, &ora_params).await
            });
            match result {
                Ok(qr) => {
                    let col_names: Vec<String> = qr.columns.iter().map(|c| c.name.clone()).collect();
                    let mut result: Vec<Value> = Vec::new();
                    for row in &qr.rows {
                        let mut m = crate::ord_map::OrdMap::new();
                        for (i, name) in col_names.iter().enumerate() {
                            let val = row.get(i).map(oracle_to_value).unwrap_or(Value::Undefined);
                            m.set(name.clone(), val);
                        }
                        result.push(Value::Map(Arc::new(Mutex::new(m))));
                    }
                    Ok(Value::Array(Arc::new(Mutex::new(result))))
                }
                Err(e) => Ok(crate::value::error_value(format!(
                    "dbQuery() 查询失败: {} (SQL: {})", e, sql,
                ))),
            }
        }
    }
}

/// bi_db_query_ordered 查询并按指定列排序。
///
/// 用法：dbQueryOrdered(conn, sql, orderByCol, order)
///   - order 为 "ASC" 或 "DESC"
///   - 在原 SQL 后追加 ORDER BY "orderByCol" "order"
///   - 返回与 dbQuery 相同格式（array of map）
///
/// 注意：orderByCol 会被双引号包裹以支持含特殊字符的列名，
/// 并对内部双引号做 SQL 转义（" → ""），防止 SQL 注入。
/// order 不区分大小写，仅接受 ASC/DESC，其他值返回错误。
fn bi_db_query_ordered(vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "dbQueryOrdered")?;
    let sql = bh::as_str(args, 1, "dbQueryOrdered")?;
    let order_col = bh::as_str(args, 2, "dbQueryOrdered")?;
    let order_raw = bh::as_str(args, 3, "dbQueryOrdered")?;

    // 校验 order 关键字（仅允许 ASC/DESC，不区分大小写）
    let order_upper = order_raw.to_uppercase();
    if order_upper != "ASC" && order_upper != "DESC" {
        return Err(crate::value::error_value(format!(
            "dbQueryOrdered() order 必须为 'ASC' 或 'DESC'，得到 '{}' (可能原因：参数顺序错误，正确顺序 dbQueryOrdered(conn, sql, orderByCol, order))",
            order_raw,
        )));
    }

    // 转义 orderByCol 中的双引号（SQL 标识符转义：每对 "" 表示一个 "）
    let escaped_col: String = order_col.chars().flat_map(|c| {
        if c == '"' { vec!['"', '"'] } else { vec![c] }
    }).collect();

    // 追加 ORDER BY 子句
    let full_sql = format!("{} ORDER BY \"{}\" {}", sql, escaped_col, order_upper);

    // 调用 bi_db_query 执行（替换原 SQL 参数）
    let new_args: Vec<Value> = vec![args[0].clone(), Value::str_from(full_sql)];
    let new_args: Vec<Value> = new_args.into_iter()
        .chain(args.get(4..).unwrap_or(&[]).iter().cloned())
        .collect();
    bi_db_query(vm, &new_args)
}

///
/// 与 dbQuery 的区别：
///   dbQuery     → [{"col1": v1, "col2": v2}, ...]（array of map）
///   dbQueryRecs → [["col1", "col2"], [v1, v2], ...]（array of array，第一行列名）
///
/// 用法：dbQueryRecs(db, sql) 或 dbQueryRecs(db, sql, param1, param2, ...)
fn bi_db_query_recs(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "dbQueryRecs")?;
    let sql = bh::as_str(args, 1, "dbQueryRecs")?;
    let db = db_downcast(&args[0], "dbQueryRecs")?;
    let params: &[Value] = if args.len() > 2 { &args[2..] } else { &[] };

    match db {
        DatabaseConn::Sqlite(conn) => {
            let guard = conn.lock().map_err(|e| crate::value::error_value(format!(
                "dbQueryRecs() 数据库锁异常: {}", e,
            )))?;
            let mut stmt = guard.prepare(sql).map_err(|e| {
                crate::value::error_value(format!("dbQueryRecs() SQL 预处理失败: {} (SQL: {})", e, sql))
            })?;
            let col_count = stmt.column_count();
            // 收集列名
            let col_names: Vec<Value> = (0..col_count)
                .map(|i| Value::str_from(stmt.column_name(i).unwrap_or_default().to_string()))
                .collect();
            let sql_params: Vec<rusqlite::types::Value> = params.iter().map(value_to_sqlite).collect();
            let rows = stmt.query_map(rusqlite::params_from_iter(sql_params.iter()), |row| {
                let mut rec: Vec<Value> = Vec::with_capacity(col_count);
                for i in 0..col_count {
                    let val: ValueRef = row.get_ref(i)?;
                    rec.push(sqlite_to_value(val));
                }
                Ok(rec)
            }).map_err(|e| {
                crate::value::error_value(format!("dbQueryRecs() 查询失败: {} (SQL: {})", e, sql))
            })?;

            let mut result: Vec<Value> = Vec::new();
            // 第一行：列名
            result.push(Value::Array(Arc::new(Mutex::new(col_names))));
            // 数据行
            for row_result in rows {
                let rec = row_result.map_err(|e| {
                    crate::value::error_value(format!("dbQueryRecs() 读取行失败: {}", e))
                })?;
                result.push(Value::Array(Arc::new(Mutex::new(rec))));
            }
            Ok(Value::Array(Arc::new(Mutex::new(result))))
        }
        DatabaseConn::Mysql(pool) => {
            use mysql::prelude::*;
            let mut conn = pool.get_conn().map_err(|e| {
                crate::value::error_value(format!("dbQueryRecs() 获取 MySQL 连接失败: {}", e))
            })?;
            let mysql_params: Vec<mysql::Value> = params.iter().map(value_to_mysql).collect();
            let mut result_set = conn.exec_iter(sql, mysql_params).map_err(|e| {
                crate::value::error_value(format!("dbQueryRecs() 查询失败: {} (SQL: {})", e, sql))
            })?;
            let cols = result_set.columns();
            let col_count = cols.as_ref().len();
            // 第一行：列名
            let col_names: Vec<Value> = cols.as_ref().iter()
                .map(|c| Value::str_from(c.name_str().to_string()))
                .collect();

            let mut result: Vec<Value> = Vec::new();
            result.push(Value::Array(Arc::new(Mutex::new(col_names))));
            for row_result in result_set.by_ref() {
                let row = row_result.map_err(|e| {
                    crate::value::error_value(format!("dbQueryRecs() 读取行失败: {}", e))
                })?;
                let values = row.unwrap();
                let mut rec: Vec<Value> = Vec::with_capacity(col_count);
                for i in 0..col_count {
                    let val = values.get(i).unwrap_or(&mysql::Value::NULL);
                    rec.push(mysql_to_value(val));
                }
                result.push(Value::Array(Arc::new(Mutex::new(rec))));
            }
            Ok(Value::Array(Arc::new(Mutex::new(result))))
        }
        DatabaseConn::Postgres(client) => {
            let mut guard = client.lock().map_err(|e| crate::value::error_value(format!(
                "dbQueryRecs() 数据库锁异常: {}", e,
            )))?;
            let pg_sql = convert_pg_placeholders(sql);
            let pg_params: Vec<Box<dyn postgres::types::ToSql + Sync>> =
                params.iter().map(value_to_pg_type).collect();
            let pg_refs: Vec<&(dyn postgres::types::ToSql + Sync)> =
                pg_params.iter().map(|b| b.as_ref()).collect();

            let rows = guard.query(&pg_sql, &pg_refs[..]).map_err(|e| {
                crate::value::error_value(format!("dbQueryRecs() 查询失败: {} (SQL: {})", e, sql))
            })?;

            let mut result: Vec<Value> = Vec::new();
            if !rows.is_empty() {
                // 第一行：列名
                let col_names: Vec<Value> = rows[0].columns().iter()
                    .map(|c| Value::str_from(c.name().to_string()))
                    .collect();
                result.push(Value::Array(Arc::new(Mutex::new(col_names))));
                // 数据行
                for row in &rows {
                    let mut rec: Vec<Value> = Vec::new();
                    for (i, col) in row.columns().iter().enumerate() {
                        rec.push(pg_get_value(row, i, col.type_()));
                    }
                    result.push(Value::Array(Arc::new(Mutex::new(rec))));
                }
            }
            Ok(Value::Array(Arc::new(Mutex::new(result))))
        }
        DatabaseConn::Mssql(client, runtime) => {
            let mut guard = client.lock().map_err(|e| crate::value::error_value(format!(
                "dbQueryRecs() 数据库锁异常: {}", e,
            )))?;
            let mssql_params: Vec<Box<dyn tiberius::ToSql>> = params.iter().map(value_to_mssql).collect();
            let param_refs: Vec<&dyn tiberius::ToSql> = mssql_params.iter().map(|b| b.as_ref()).collect();

            let result = runtime.block_on(async {
                let stream = if param_refs.is_empty() {
                    guard.simple_query(sql).await
                } else {
                    guard.query(sql, &param_refs).await
                };
                match stream {
                    Ok(s) => {
                        use futures_util::TryStreamExt;
                        use tiberius::QueryItem;
                        let items: Vec<QueryItem> = s.try_collect().await?;
                        let rows: Vec<tiberius::Row> = items.into_iter().filter_map(|item| {
                            if let QueryItem::Row(row) = item { Some(row) } else { None }
                        }).collect();
                        Ok(rows)
                    }
                    Err(e) => Err(e),
                }
            });

            match result {
                Ok(rows) => {
                    let mut result: Vec<Value> = Vec::new();
                    if !rows.is_empty() {
                        // 第一行：列名
                        let col_names: Vec<Value> = rows[0].columns().iter()
                            .map(|c| Value::str_from(c.name().to_string()))
                            .collect();
                        result.push(Value::Array(Arc::new(Mutex::new(col_names))));
                        // 数据行
                        for row in &rows {
                            let mut rec: Vec<Value> = Vec::new();
                            for col in row.columns() {
                                rec.push(mssql_get_row_value(row, col));
                            }
                            result.push(Value::Array(Arc::new(Mutex::new(rec))));
                        }
                    }
                    Ok(Value::Array(Arc::new(Mutex::new(result))))
                }
                Err(e) => Ok(crate::value::error_value(format!(
                    "dbQueryRecs() 查询失败: {} (SQL: {})", e, sql,
                ))),
            }
        }
        DatabaseConn::Oracle(conn, runtime) => {
            let guard = conn.lock().map_err(|e| crate::value::error_value(format!(
                "dbQueryRecs() 数据库锁异常: {}", e,
            )))?;
            let ora_params: Vec<oracle_rs::row::Value> = params.iter().map(sflang_to_oracle).collect();
            let result = runtime.block_on(async {
                guard.query(sql, &ora_params).await
            });
            match result {
                Ok(qr) => {
                    let mut result: Vec<Value> = Vec::new();
                    if !qr.rows.is_empty() {
                        // 第一行：列名
                        let col_names: Vec<Value> = qr.columns.iter()
                            .map(|c| Value::str_from(c.name.clone()))
                            .collect();
                        result.push(Value::Array(Arc::new(Mutex::new(col_names))));
                        // 数据行
                        for row in &qr.rows {
                            let mut rec: Vec<Value> = Vec::new();
                            for i in 0..qr.columns.len() {
                                let val = row.get(i).map(oracle_to_value).unwrap_or(Value::Undefined);
                                rec.push(val);
                            }
                            result.push(Value::Array(Arc::new(Mutex::new(rec))));
                        }
                    }
                    Ok(Value::Array(Arc::new(Mutex::new(result))))
                }
                Err(e) => Ok(crate::value::error_value(format!(
                    "dbQueryRecs() 查询失败: {} (SQL: {})", e, sql,
                ))),
            }
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
        DatabaseConn::Postgres(_) => {}
        DatabaseConn::Mssql(_, _) => {}
        DatabaseConn::Oracle(_, _) => {}
    }
    Ok(Value::Undefined)
}

// ---- 便捷查询函数 ----
//
// 这 5 个函数都是对 dbQuery 的一层薄封装，提取常用模式。
// 所有参数绑定的逻辑与 dbQuery 完全一致（复用内部查询逻辑）。

/// db_query_scalar 执行查询并返回第一行第一列的值（内部辅助）。
///
/// 用于 dbQueryCount/Float/String 的公共逻辑。
fn db_query_scalar(vm: &mut crate::vm::VM, fn_name: &str, args: &[Value]) -> Result<Value, Value> {
    // 复用 dbQuery 的逻辑
    let rows = bi_db_query(vm, args)?;
    match &rows {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            if g.is_empty() {
                return Ok(Value::Undefined);
            }
            match &g[0] {
                Value::Map(m) => {
                    let mg = m.lock().unwrap();
                    // 取第一个值
                    match mg.values().into_iter().next() {
                        Some(v) => Ok(v.clone()),
                        None => Ok(Value::Undefined),
                    }
                }
                _ => Ok(Value::Undefined),
            }
        }
        _ => Err(crate::value::error_value(format!(
            "{}() 内部错误: dbQuery 返回了非数组类型", fn_name,
        ))),
    }
}

/// bi_db_query_count 执行 COUNT 查询，返回整数。
///
/// 用法：dbQueryCount(db, sql, params...)
/// 示例：dbQueryCount(db, "SELECT COUNT(*) FROM users WHERE age > ?", 18)
fn bi_db_query_count(vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    let v = db_query_scalar(vm, "dbQueryCount", args)?;
    match v {
        Value::Int(_) => Ok(v),
        Value::Float(f) => Ok(Value::Int(f as i64)),
        Value::Str(s) => Ok(s.parse::<i64>().map(Value::Int).unwrap_or(Value::Int(0))),
        Value::Undefined => Ok(Value::Int(0)),
        other => Ok(Value::Int(other.to_int().unwrap_or(0))),
    }
}

/// bi_db_query_float 执行查询，返回单个浮点值。
///
/// 用法：dbQueryFloat(db, sql, params...)
/// 示例：dbQueryFloat(db, "SELECT AVG(score) FROM students WHERE class = ?", "A")
fn bi_db_query_float(vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    let v = db_query_scalar(vm, "dbQueryFloat", args)?;
    match v {
        Value::Float(_) => Ok(v),
        Value::Int(i) => Ok(Value::Float(i as f64)),
        Value::Str(s) => Ok(s.parse::<f64>().map(Value::Float).unwrap_or(Value::Float(0.0))),
        Value::Undefined => Ok(Value::Float(0.0)),
        other => match other.to_f64() {
            Some(f) => Ok(Value::Float(f)),
            None => Ok(Value::Float(0.0)),
        },
    }
}

/// bi_db_query_string 执行查询，返回单个字符串值。
///
/// 用法：dbQueryString(db, sql, params...)
/// 示例：dbQueryString(db, "SELECT name FROM users WHERE id = ?", 1)
fn bi_db_query_string(vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    let v = db_query_scalar(vm, "dbQueryString", args)?;
    Ok(Value::str_from(v.to_str()))
}

/// bi_db_query_map 执行查询，按指定列的值作为 key 组织为 map（一对一）。
///
/// 用法：dbQueryMap(db, keyColumn, sql, params...)
/// 结果: {keyValue: {col1: v1, col2: v2, ...}, ...}
///
/// 示例：
///   dbQueryMap(db, "id", "SELECT id, name, age FROM users")
///   → {"1": {"name":"Alice","age":30}, "2": {"name":"Bob","age":25}}
fn bi_db_query_map(vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "dbQueryMap")?;
    let key_col = bh::as_str(args, 1, "dbQueryMap")?;
    // 把 db 和 sql 之后的参数传给 dbQuery（args[0]=db, args[2]=sql, args[3..]=params）
    let query_args: Vec<Value> = vec![args[0].clone(), args[2].clone()];
    let query_args: Vec<Value> = query_args.into_iter()
        .chain(args.get(3..).unwrap_or(&[]).iter().cloned())
        .collect();
    let rows = bi_db_query(vm, &query_args)?;

    let mut result = crate::ord_map::OrdMap::new();
    if let Value::Array(a) = &rows {
        for row_val in a.lock().unwrap().iter() {
            if let Value::Map(m) = row_val {
                let mg = m.lock().unwrap();
                if let Some(key_val) = mg.get(key_col) {
                    let key = key_val.to_str();
                    result.set(key, row_val.clone());
                }
            }
        }
    }
    Ok(Value::Map(Arc::new(Mutex::new(result))))
}

/// bi_db_query_map_array 执行查询，按指定列的值作为 key 组织为 map（一对多）。
///
/// 用法：dbQueryMapArray(db, keyColumn, sql, params...)
/// 结果: {keyValue: [row1, row2, ...], ...}
///
/// 示例：
///   dbQueryMapArray(db, "dept", "SELECT dept, name FROM employees")
///   → {"销售部": [{"name":"张三"}, ...], "技术部": [{"name":"王五"}, ...]}
fn bi_db_query_map_array(vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "dbQueryMapArray")?;
    let key_col = bh::as_str(args, 1, "dbQueryMapArray")?;
    let query_args: Vec<Value> = vec![args[0].clone(), args[2].clone()];
    let query_args: Vec<Value> = query_args.into_iter()
        .chain(args.get(3..).unwrap_or(&[]).iter().cloned())
        .collect();
    let rows = bi_db_query(vm, &query_args)?;

    let mut result = crate::ord_map::OrdMap::new();
    if let Value::Array(a) = &rows {
        for row_val in a.lock().unwrap().iter() {
            if let Value::Map(m) = row_val {
                let mg = m.lock().unwrap();
                if let Some(key_val) = mg.get(key_col) {
                    let key = key_val.to_str();
                    // 追加到数组（不存在则创建）
                    let existing = result.get(&key);
                    let mut arr = match existing {
                        Some(Value::Array(a)) => a.lock().unwrap().clone(),
                        _ => Vec::new(),
                    };
                    arr.push(row_val.clone());
                    result.set(key, Value::Array(Arc::new(Mutex::new(arr))));
                }
            }
        }
    }
    Ok(Value::Map(Arc::new(Mutex::new(result))))
}

/// bi_format_sql_value 将 Value 转为 SQL 字面量字符串。
///
/// 用法：formatSqlValue(v) → string
///
/// 转换规则：
///   - int / byte: 直接转字符串（如 42）
///   - float: 直接转字符串（如 3.14）
///   - string: 用单引号包裹，内部单引号转义为两个单引号（' → ''）
///   - bool: TRUE / FALSE
///   - undefined / error: NULL
///
/// 用于构造 SQL 语句的值字面量（替代参数化查询的便捷方式）。
/// 注意：直接拼接 SQL 字面量存在 SQL 注入风险，推荐使用 dbExec/dbQuery
/// 的参数化查询；formatSqlValue 主要用于生成 DDL/调试等场景。
fn bi_format_sql_value(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "formatSqlValue")?;
    let v = &args[0];
    let s = match v {
        Value::Int(i) => i.to_string(),
        Value::Byte(b) => b.to_string(),
        Value::Float(f) => {
            // 整数浮点也保留小数形式（与 SQL 语义一致）
            format!("{}", f)
        }
        Value::BigInt(b) => b.to_string_decimal(),
        Value::Str(s) => {
            // 单引号转义为两个单引号
            let escaped: String = s.chars().flat_map(|c| {
                if c == '\'' { vec!['\'', '\''] } else { vec![c] }
            }).collect();
            format!("'{}'", escaped)
        }
        Value::Bool(b) => if *b { "TRUE".to_string() } else { "FALSE".to_string() },
        Value::Undefined | Value::Error(_) => "NULL".to_string(),
        // 其他类型降级为字符串字面量（按 to_str）
        other => {
            let s = other.to_str();
            let escaped: String = s.chars().flat_map(|c| {
                if c == '\'' { vec!['\'', '\''] } else { vec![c] }
            }).collect();
            format!("'{}'", escaped)
        }
    };
    Ok(Value::str_from(s))
}
