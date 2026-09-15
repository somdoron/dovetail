fn main() {
    println!("cargo:rerun-if-changed=sqlite3/sqlite3.c");
    println!("cargo:rerun-if-changed=sqlite3/sqlite3.h");
    cc::Build::new()
        .file("sqlite3/sqlite3.c")
        // __wasi__ (defined by wasi clang) already forces SQLITE_WASI,
        // SQLITE_THREADSAFE=0 and SQLITE_OMIT_LOAD_EXTENSION inside the
        // amalgamation; the defines below trim the build further.
        .define("SQLITE_OMIT_DEPRECATED", None)
        .define("SQLITE_OMIT_SHARED_CACHE", None)
        .define("SQLITE_DEFAULT_MEMSTATUS", "0")
        .define("SQLITE_OMIT_WAL", None)
        .define("SQLITE_USE_URI", "1")
        .flag_if_supported("-Wno-unused-parameter")
        .compile("sqlite3");
}
