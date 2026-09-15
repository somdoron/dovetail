//! Hand-written async-stackful shim for the dovetail:sqlite-raw/raw WIT
//! interface. NO wit-bindgen: every export is lifted `[async-lift-stackful]`
//! so SQLite's synchronous, potentially-blocking p2 filesystem I/O runs on the
//! export's own fiber and suspends there, instead of stalling the caller's
//! store. Each export does its work, lowers the result
//! into linear memory per the canonical ABI, and delivers it via the
//! `[task-return]` builtin.
//!
//! `database`/`statement` are opaque `u64` handles (raw C pointers), not
//! component resources, so no resource canonical-ABI intrinsics are needed.
#![allow(clippy::missing_safety_doc)]

use core::ffi::{c_char, c_int, c_uchar, c_void};

// --- libc (wasi-libc, linked via the wasip1 sysroot) ---------------------

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
}

// --- Raw SQLite C API (subset used by the shim) --------------------------

#[allow(non_camel_case_types)]
type sqlite3 = c_void;
#[allow(non_camel_case_types)]
type sqlite3_stmt = c_void;

/// SQLITE_TRANSIENT: make SQLite copy bound text/blob payloads.
const SQLITE_TRANSIENT: isize = -1;
const SQLITE_MISUSE: c_int = 21;

unsafe extern "C" {
    fn sqlite3_open_v2(
        filename: *const c_char,
        db: *mut *mut sqlite3,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close_v2(db: *mut sqlite3) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut sqlite3,
        sql: *const c_char,
        n_byte: c_int,
        stmt: *mut *mut sqlite3_stmt,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_finalize(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
    fn sqlite3_extended_errcode(db: *mut sqlite3) -> c_int;
    fn sqlite3_last_insert_rowid(db: *mut sqlite3) -> i64;
    fn sqlite3_changes64(db: *mut sqlite3) -> i64;
    fn sqlite3_busy_timeout(db: *mut sqlite3, ms: c_int) -> c_int;
    fn sqlite3_step(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_reset(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_clear_bindings(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_bind_null(stmt: *mut sqlite3_stmt, index: c_int) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut sqlite3_stmt, index: c_int, value: i64) -> c_int;
    fn sqlite3_bind_double(stmt: *mut sqlite3_stmt, index: c_int, value: f64) -> c_int;
    fn sqlite3_bind_text(
        stmt: *mut sqlite3_stmt,
        index: c_int,
        value: *const c_char,
        n_byte: c_int,
        destructor: isize,
    ) -> c_int;
    fn sqlite3_bind_blob(
        stmt: *mut sqlite3_stmt,
        index: c_int,
        value: *const c_void,
        n_byte: c_int,
        destructor: isize,
    ) -> c_int;
    fn sqlite3_column_count(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_column_type(stmt: *mut sqlite3_stmt, index: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: *mut sqlite3_stmt, index: c_int) -> i64;
    fn sqlite3_column_double(stmt: *mut sqlite3_stmt, index: c_int) -> f64;
    fn sqlite3_column_text(stmt: *mut sqlite3_stmt, index: c_int) -> *const c_uchar;
    fn sqlite3_column_blob(stmt: *mut sqlite3_stmt, index: c_int) -> *const c_void;
    fn sqlite3_column_bytes(stmt: *mut sqlite3_stmt, index: c_int) -> c_int;
    fn sqlite3_column_name(stmt: *mut sqlite3_stmt, index: c_int) -> *const c_char;
}

// --- Canonical-ABI intrinsics --------------------------------------------

/// Component-ABI allocator the host calls to place string/list params into our
/// linear memory (and that we reuse for lowering results). Backed by libc.
#[export_name = "cabi_realloc"]
pub unsafe extern "C" fn cabi_realloc(
    old: *mut u8,
    _old_size: usize,
    _align: usize,
    new_size: usize,
) -> *mut u8 {
    unsafe {
        if old.is_null() {
            malloc(new_size) as *mut u8
        } else {
            realloc(old as *mut c_void, new_size) as *mut u8
        }
    }
}

/// `[task-return]` builtins — one per exported function, resolved by
/// `wasm-tools component new` when it lifts the async exports. Signatures
/// match the function's lowered result: a single flat value, a pointer to the
/// lowered aggregate (result/string/list), or nothing.
#[link(wasm_import_module = "[export]dovetail:sqlite-raw/raw@0.1.0")]
unsafe extern "C" {
    #[link_name = "[task-return]open"]
    fn tr_open(disc: i32, payload: i64);
    #[link_name = "[task-return]prepare"]
    fn tr_prepare(disc: i32, payload: i64);
    #[link_name = "[task-return]errmsg"]
    fn tr_errmsg(ptr: i32, len: i32);
    #[link_name = "[task-return]extended-errcode"]
    fn tr_extended_errcode(v: i32);
    #[link_name = "[task-return]last-insert-rowid"]
    fn tr_last_insert_rowid(v: i64);
    #[link_name = "[task-return]changes"]
    fn tr_changes(v: i64);
    #[link_name = "[task-return]busy-timeout"]
    fn tr_busy_timeout(v: i32);
    #[link_name = "[task-return]db-close"]
    fn tr_db_close();
    #[link_name = "[task-return]step"]
    fn tr_step(v: i32);
    #[link_name = "[task-return]reset"]
    fn tr_reset(v: i32);
    #[link_name = "[task-return]clear-bindings"]
    fn tr_clear_bindings(v: i32);
    #[link_name = "[task-return]bind-null"]
    fn tr_bind_null(v: i32);
    #[link_name = "[task-return]bind-int64"]
    fn tr_bind_int64(v: i32);
    #[link_name = "[task-return]bind-double"]
    fn tr_bind_double(v: i32);
    #[link_name = "[task-return]bind-text"]
    fn tr_bind_text(v: i32);
    #[link_name = "[task-return]bind-blob"]
    fn tr_bind_blob(v: i32);
    #[link_name = "[task-return]column-count"]
    fn tr_column_count(v: i32);
    #[link_name = "[task-return]column-type"]
    fn tr_column_type(v: i32);
    #[link_name = "[task-return]column-int64"]
    fn tr_column_int64(v: i64);
    #[link_name = "[task-return]column-double"]
    fn tr_column_double(v: f64);
    #[link_name = "[task-return]column-text"]
    fn tr_column_text(ptr: i32, len: i32);
    #[link_name = "[task-return]column-blob"]
    fn tr_column_blob(ptr: i32, len: i32);
    #[link_name = "[task-return]column-name"]
    fn tr_column_name(ptr: i32, len: i32);
    #[link_name = "[task-return]stmt-finalize"]
    fn tr_stmt_finalize();
}

// --- Result lowering helpers ---------------------------------------------
//
// `task.return` receives results FLATTENED (not via a return pointer):
// `result<u64,s32>` as (disc: i32, payload: i64), a string/list as
// (ptr: i32, len: i32). The disc is 0=ok / 1=err; the payload holds the u64
// handle (ok) or the s32 code in its low bits (err).

/// Copy a byte payload (string/list) into a fresh linear-memory allocation the
/// host can lift, returning `(ptr, len)`. Empty payloads still get a non-null
/// pointer. The allocation leaks (no cabi_post) — acceptable for the shim.
unsafe fn copy_bytes(bytes: &[u8]) -> (i32, i32) {
    unsafe {
        let n = bytes.len();
        let data = if n == 0 {
            malloc(1) as *mut u8
        } else {
            let d = malloc(n) as *mut u8;
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), d, n);
            d
        };
        (data as i32, n as i32)
    }
}

/// Deliver a `result<u64, s32>` via the given task-return.
unsafe fn return_result(tr: unsafe extern "C" fn(i32, i64), r: Result<u64, i32>) {
    unsafe {
        match r {
            Ok(h) => tr(0, h as i64),
            Err(code) => tr(1, code as i64),
        }
    }
}

// --- Exports (async-stackful) --------------------------------------------

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#open"]
pub unsafe extern "C" fn export_open(path_ptr: *const u8, path_len: usize, flags: i32) {
    unsafe {
        // sqlite3_open_v2 needs a NUL-terminated path; copy + terminate.
        let cpath = malloc(path_len + 1) as *mut u8;
        core::ptr::copy_nonoverlapping(path_ptr, cpath, path_len);
        *cpath.add(path_len) = 0;
        let mut db: *mut sqlite3 = core::ptr::null_mut();
        let rc = sqlite3_open_v2(cpath as *const c_char, &mut db, flags, core::ptr::null());
        realloc(cpath as *mut c_void, 0);
        let res = if rc != 0 {
            if !db.is_null() {
                sqlite3_close_v2(db);
            }
            Err(rc)
        } else {
            Ok(db as u64)
        };
        return_result(tr_open, res);
    }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#prepare"]
pub unsafe extern "C" fn export_prepare(db: i64, sql_ptr: *const u8, sql_len: usize) {
    unsafe {
        let mut stmt: *mut sqlite3_stmt = core::ptr::null_mut();
        let rc = sqlite3_prepare_v2(
            db as *mut sqlite3,
            sql_ptr as *const c_char,
            sql_len as c_int,
            &mut stmt,
            core::ptr::null_mut(),
        );
        let res = if rc != 0 {
            Err(rc)
        } else if stmt.is_null() {
            // Whitespace/comment-only SQL: no statement to represent.
            Err(SQLITE_MISUSE)
        } else {
            Ok(stmt as u64)
        };
        return_result(tr_prepare, res);
    }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#errmsg"]
pub unsafe extern "C" fn export_errmsg(db: i64) {
    unsafe {
        let ptr = sqlite3_errmsg(db as *mut sqlite3);
        let (p, l) = copy_bytes(cstr_bytes(ptr));
        tr_errmsg(p, l);
    }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#extended-errcode"]
pub unsafe extern "C" fn export_extended_errcode(db: i64) {
    unsafe { tr_extended_errcode(sqlite3_extended_errcode(db as *mut sqlite3)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#last-insert-rowid"]
pub unsafe extern "C" fn export_last_insert_rowid(db: i64) {
    unsafe { tr_last_insert_rowid(sqlite3_last_insert_rowid(db as *mut sqlite3)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#changes"]
pub unsafe extern "C" fn export_changes(db: i64) {
    unsafe { tr_changes(sqlite3_changes64(db as *mut sqlite3)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#busy-timeout"]
pub unsafe extern "C" fn export_busy_timeout(db: i64, ms: i32) {
    unsafe { tr_busy_timeout(sqlite3_busy_timeout(db as *mut sqlite3, ms)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#db-close"]
pub unsafe extern "C" fn export_db_close(db: i64) {
    unsafe {
        sqlite3_close_v2(db as *mut sqlite3);
        tr_db_close();
    }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#step"]
pub unsafe extern "C" fn export_step(stmt: i64) {
    unsafe { tr_step(sqlite3_step(stmt as *mut sqlite3_stmt)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#reset"]
pub unsafe extern "C" fn export_reset(stmt: i64) {
    unsafe { tr_reset(sqlite3_reset(stmt as *mut sqlite3_stmt)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#clear-bindings"]
pub unsafe extern "C" fn export_clear_bindings(stmt: i64) {
    unsafe { tr_clear_bindings(sqlite3_clear_bindings(stmt as *mut sqlite3_stmt)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#bind-null"]
pub unsafe extern "C" fn export_bind_null(stmt: i64, index: i32) {
    unsafe { tr_bind_null(sqlite3_bind_null(stmt as *mut sqlite3_stmt, index)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#bind-int64"]
pub unsafe extern "C" fn export_bind_int64(stmt: i64, index: i32, value: i64) {
    unsafe { tr_bind_int64(sqlite3_bind_int64(stmt as *mut sqlite3_stmt, index, value)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#bind-double"]
pub unsafe extern "C" fn export_bind_double(stmt: i64, index: i32, value: f64) {
    unsafe { tr_bind_double(sqlite3_bind_double(stmt as *mut sqlite3_stmt, index, value)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#bind-text"]
pub unsafe extern "C" fn export_bind_text(stmt: i64, index: i32, ptr: *const u8, len: usize) {
    unsafe {
        let rc = sqlite3_bind_text(
            stmt as *mut sqlite3_stmt,
            index,
            ptr as *const c_char,
            len as c_int,
            SQLITE_TRANSIENT,
        );
        tr_bind_text(rc);
    }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#bind-blob"]
pub unsafe extern "C" fn export_bind_blob(stmt: i64, index: i32, ptr: *const u8, len: usize) {
    unsafe {
        let rc = sqlite3_bind_blob(
            stmt as *mut sqlite3_stmt,
            index,
            ptr as *const c_void,
            len as c_int,
            SQLITE_TRANSIENT,
        );
        tr_bind_blob(rc);
    }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#column-count"]
pub unsafe extern "C" fn export_column_count(stmt: i64) {
    unsafe { tr_column_count(sqlite3_column_count(stmt as *mut sqlite3_stmt)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#column-type"]
pub unsafe extern "C" fn export_column_type(stmt: i64, index: i32) {
    unsafe { tr_column_type(sqlite3_column_type(stmt as *mut sqlite3_stmt, index)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#column-int64"]
pub unsafe extern "C" fn export_column_int64(stmt: i64, index: i32) {
    unsafe { tr_column_int64(sqlite3_column_int64(stmt as *mut sqlite3_stmt, index)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#column-double"]
pub unsafe extern "C" fn export_column_double(stmt: i64, index: i32) {
    unsafe { tr_column_double(sqlite3_column_double(stmt as *mut sqlite3_stmt, index)) }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#column-text"]
pub unsafe extern "C" fn export_column_text(stmt: i64, index: i32) {
    unsafe {
        let s = stmt as *mut sqlite3_stmt;
        let ptr = sqlite3_column_text(s, index);
        let bytes: &[u8] = if ptr.is_null() {
            &[]
        } else {
            let len = sqlite3_column_bytes(s, index) as usize;
            core::slice::from_raw_parts(ptr, len)
        };
        let (p, l) = copy_bytes(bytes);
        tr_column_text(p, l);
    }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#column-blob"]
pub unsafe extern "C" fn export_column_blob(stmt: i64, index: i32) {
    unsafe {
        let s = stmt as *mut sqlite3_stmt;
        let ptr = sqlite3_column_blob(s, index);
        let bytes: &[u8] = if ptr.is_null() {
            &[]
        } else {
            let len = sqlite3_column_bytes(s, index) as usize;
            core::slice::from_raw_parts(ptr as *const u8, len)
        };
        let (p, l) = copy_bytes(bytes);
        tr_column_blob(p, l);
    }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#column-name"]
pub unsafe extern "C" fn export_column_name(stmt: i64, index: i32) {
    unsafe {
        let ptr = sqlite3_column_name(stmt as *mut sqlite3_stmt, index);
        let (p, l) = copy_bytes(cstr_bytes(ptr));
        tr_column_name(p, l);
    }
}

#[export_name = "[async-lift-stackful]dovetail:sqlite-raw/raw@0.1.0#stmt-finalize"]
pub unsafe extern "C" fn export_stmt_finalize(stmt: i64) {
    unsafe {
        sqlite3_finalize(stmt as *mut sqlite3_stmt);
        tr_stmt_finalize();
    }
}

/// Bytes of a NUL-terminated C string (excluding the terminator), or empty.
unsafe fn cstr_bytes<'a>(ptr: *const c_char) -> &'a [u8] {
    unsafe {
        if ptr.is_null() {
            return &[];
        }
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        core::slice::from_raw_parts(ptr as *const u8, len)
    }
}
