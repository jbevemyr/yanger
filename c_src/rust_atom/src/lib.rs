use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::slice;
use std::sync::{Mutex, OnceLock};

mod core_types;
mod core_validators;
mod grammar;
mod parser;

struct AtomTable {
    by_value: HashMap<Vec<u8>, *mut c_char>,
}

impl AtomTable {
    fn new() -> Self {
        Self {
            by_value: HashMap::new(),
        }
    }

    fn intern_bytes(&mut self, bytes: &[u8]) -> *mut c_char {
        if let Some(existing) = self.by_value.get(bytes) {
            return *existing;
        }

        let mut clean = Vec::with_capacity(bytes.len());
        for b in bytes {
            if *b != 0 {
                clean.push(*b);
            }
        }

        let cstr = match CString::new(clean.clone()) {
            Ok(v) => v,
            Err(_) => return ptr::null_mut(),
        };

        let ptr = cstr.into_raw();
        self.by_value.insert(clean, ptr);
        ptr
    }
}

// Accessed from C; mutation is protected by the mutex.
unsafe impl Send for AtomTable {}

static ATOMS: OnceLock<Mutex<AtomTable>> = OnceLock::new();

fn atoms() -> &'static Mutex<AtomTable> {
    ATOMS.get_or_init(|| Mutex::new(AtomTable::new()))
}

#[no_mangle]
pub unsafe extern "C" fn yang_make_atom_len(str_ptr: *const c_char, len: c_int) -> *mut c_char {
    if str_ptr.is_null() || len < 0 {
        return ptr::null_mut();
    }

    let bytes = slice::from_raw_parts(str_ptr as *const u8, len as usize);
    let mut guard = match atoms().lock() {
        Ok(g) => g,
        Err(_) => return ptr::null_mut(),
    };
    guard.intern_bytes(bytes)
}

#[no_mangle]
pub unsafe extern "C" fn yang_make_atom(str_ptr: *const c_char) -> *mut c_char {
    if str_ptr.is_null() {
        return ptr::null_mut();
    }
    let cstr = CStr::from_ptr(str_ptr);
    let mut guard = match atoms().lock() {
        Ok(g) => g,
        Err(_) => return ptr::null_mut(),
    };
    guard.intern_bytes(cstr.to_bytes())
}

#[no_mangle]
pub unsafe extern "C" fn yang_is_atom_len(str_ptr: *const c_char, _len: c_int) -> c_int {
    if str_ptr.is_null() {
        return 0;
    }

    let guard = match atoms().lock() {
        Ok(g) => g,
        Err(_) => return 0,
    };

    for atom_ptr in guard.by_value.values() {
        if *atom_ptr == str_ptr as *mut c_char {
            return 1;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn yang_is_atom(str_ptr: *const c_char) -> c_int {
    yang_is_atom_len(str_ptr, 0)
}

#[no_mangle]
pub unsafe extern "C" fn yang_print_atom_table() {
    let guard = match atoms().lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    for atom_ptr in guard.by_value.values() {
        if atom_ptr.is_null() {
            continue;
        }
        let cstr = CStr::from_ptr(*atom_ptr);
        println!("atom: {}", cstr.to_string_lossy());
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tok {
    SyntaxError,
    Eof,
    Or,
    And,
    Not,
    LParen,
    RParen,
    Identifier,
}

#[derive(Clone)]
struct Lexer<'a> {
    bytes: &'a [u8],
    pos: usize,
    peek_tok: Option<Tok>,
}

fn is_identifier1(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_identifier2(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-'
}

fn is_wspace(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
}

fn is_delim(b: Option<u8>) -> bool {
    matches!(b, None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'(' | b')'))
}

fn get_tok_impl(lex: &mut Lexer<'_>) -> Tok {
    while let Some(b) = lex.bytes.get(lex.pos) {
        if is_wspace(*b) {
            lex.pos += 1;
        } else {
            break;
        }
    }

    let Some(&b) = lex.bytes.get(lex.pos) else {
        return Tok::Eof;
    };

    if b == b'(' {
        lex.pos += 1;
        return Tok::LParen;
    }
    if b == b')' {
        lex.pos += 1;
        return Tok::RParen;
    }

    if lex.bytes.len().saturating_sub(lex.pos) >= 2
        && &lex.bytes[lex.pos..lex.pos + 2] == b"or"
        && is_delim(lex.bytes.get(lex.pos + 2).copied())
    {
        lex.pos += 2;
        return Tok::Or;
    }
    if lex.bytes.len().saturating_sub(lex.pos) >= 3
        && &lex.bytes[lex.pos..lex.pos + 3] == b"and"
        && is_delim(lex.bytes.get(lex.pos + 3).copied())
    {
        lex.pos += 3;
        return Tok::And;
    }
    if lex.bytes.len().saturating_sub(lex.pos) >= 3
        && &lex.bytes[lex.pos..lex.pos + 3] == b"not"
        && is_delim(lex.bytes.get(lex.pos + 3).copied())
    {
        lex.pos += 3;
        return Tok::Not;
    }

    if !is_identifier1(b) {
        return Tok::SyntaxError;
    }

    lex.pos += 1;
    while let Some(&c) = lex.bytes.get(lex.pos) {
        if is_identifier2(c) {
            lex.pos += 1;
        } else {
            break;
        }
    }

    if lex.bytes.get(lex.pos) == Some(&b':') {
        lex.pos += 1;
        let Some(&next) = lex.bytes.get(lex.pos) else {
            return Tok::SyntaxError;
        };
        if !is_identifier1(next) {
            return Tok::SyntaxError;
        }
        lex.pos += 1;
        while let Some(&c) = lex.bytes.get(lex.pos) {
            if is_identifier2(c) {
                lex.pos += 1;
            } else {
                break;
            }
        }
    }

    Tok::Identifier
}

fn get_tok(lex: &mut Lexer<'_>) -> Tok {
    if let Some(tok) = lex.peek_tok.take() {
        return tok;
    }
    get_tok_impl(lex)
}

fn push_tok(lex: &mut Lexer<'_>, tok: Tok) {
    lex.peek_tok = Some(tok);
}

fn parse_y(lex: &mut Lexer<'_>) -> bool {
    match get_tok(lex) {
        Tok::Not => {
            let checkpoint = lex.clone();
            if !parse_x(lex) {
                // Treat "not" as identifier when expression otherwise fails.
                *lex = checkpoint;
                true
            } else {
                true
            }
        }
        Tok::LParen => {
            if !parse_x(lex) {
                return false;
            }
            matches!(get_tok(lex), Tok::RParen)
        }
        Tok::Identifier | Tok::Or | Tok::And => true,
        _ => false,
    }
}

fn parse_x(lex: &mut Lexer<'_>) -> bool {
    if !parse_y(lex) {
        return false;
    }
    let mut tok = get_tok(lex);
    while matches!(tok, Tok::And | Tok::Or) {
        if !parse_y(lex) {
            return false;
        }
        tok = get_tok(lex);
    }
    push_tok(lex, tok);
    true
}

#[no_mangle]
pub unsafe extern "C" fn yang_parse_if_feature_expr(str_ptr: *mut c_char) -> bool {
    if str_ptr.is_null() {
        return false;
    }

    let cstr = CStr::from_ptr(str_ptr);
    let bytes = cstr.to_bytes();
    let mut lex = Lexer {
        bytes,
        pos: 0,
        peek_tok: None,
    };

    parse_x(&mut lex) && matches!(get_tok(&mut lex), Tok::Eof)
}

#[repr(C)]
pub struct YangError {
    code: c_int,
    filename: *const c_char,
    line: c_int,
    col: c_int,
    msg: [c_char; libc::BUFSIZ as usize],
    next: *mut YangError,
}

#[repr(C)]
pub struct YangErrorCtx {
    err: *mut YangError,
}

unsafe fn add_err_node(ectx: *mut YangErrorCtx) -> *mut YangError {
    if ectx.is_null() {
        return ptr::null_mut();
    }
    let boxed = Box::new(YangError {
        code: 0,
        filename: ptr::null(),
        line: 0,
        col: 0,
        msg: [0; libc::BUFSIZ as usize],
        next: (*ectx).err,
    });
    let ptr = Box::into_raw(boxed);
    (*ectx).err = ptr;
    ptr
}

#[no_mangle]
pub unsafe extern "C" fn yang_alloc_err_ctx_rs() -> *mut YangErrorCtx {
    Box::into_raw(Box::new(YangErrorCtx {
        err: ptr::null_mut(),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn yang_free_err_ctx_rs(ectx: *mut YangErrorCtx) {
    if ectx.is_null() {
        return;
    }
    let mut p = (*ectx).err;
    while !p.is_null() {
        let next = (*p).next;
        drop(Box::from_raw(p));
        p = next;
    }
    drop(Box::from_raw(ectx));
}

#[no_mangle]
pub unsafe extern "C" fn yang_alloc_err_ctx() -> *mut YangErrorCtx {
    yang_alloc_err_ctx_rs()
}

#[no_mangle]
pub unsafe extern "C" fn yang_free_err_ctx(ectx: *mut YangErrorCtx) {
    yang_free_err_ctx_rs(ectx)
}

#[no_mangle]
pub unsafe extern "C" fn yang_add_err_msg_rs(
    ectx: *mut YangErrorCtx,
    code: c_int,
    filename: *const c_char,
    line: c_int,
    col: c_int,
    msg: *const c_char,
) {
    let err = add_err_node(ectx);
    if err.is_null() {
        return;
    }

    (*err).code = code;
    (*err).filename = filename;
    (*err).line = line;
    (*err).col = col;

    let mut tmp = [0_u8; libc::BUFSIZ as usize];
    let copied = if msg.is_null() {
        0
    } else {
        let source = CStr::from_ptr(msg).to_bytes();
        let n = source.len().min(tmp.len().saturating_sub(1));
        tmp[..n].copy_from_slice(&source[..n]);
        n
    };
    tmp[copied] = 0;

    let dst = (*err).msg.as_mut_ptr() as *mut u8;
    ptr::copy_nonoverlapping(tmp.as_ptr(), dst, tmp.len());
}
