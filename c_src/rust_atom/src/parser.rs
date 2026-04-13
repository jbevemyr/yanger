use crate::{yang_add_err_msg_rs, yang_make_atom, yang_make_atom_len, YangErrorCtx};
use std::ffi::{CStr, CString, OsStr};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::raw::{c_char, c_int};
use std::os::unix::ffi::OsStrExt;
use std::ptr;
use std::sync::OnceLock;

const YANG_VERSION_1: i8 = 1;
const YANG_VERSION_1_1: i8 = 2;

const YANG_ERR_PARSE_EOF: c_int = 100;
const YANG_ERR_PARSE_FOPEN: c_int = 102;
const YANG_ERR_PARSE_BAD_KEYWORD: c_int = 103;
const YANG_ERR_PARSE_EXPECTED_SEPARATOR: c_int = 104;
const YANG_ERR_PARSE_EXPECTED_STRING: c_int = 105;
const YANG_ERR_PARSE_INCOMPLETE_STATEMENT: c_int = 106;
const YANG_ERR_PARSE_EXPECTED_QUOTED_STRING: c_int = 107;
const YANG_ERR_PARSE_TRAILING_GARBAGE: c_int = 108;
const YANG_ERR_PARSE_ILLEGAL_ESCAPE: c_int = 109;
const YANG_WARN_PARSE_ILLEGAL_ESCAPE: c_int = 1000;

type YangAtom = *mut c_char;

#[repr(C)]
pub struct YangStatement {
    pub prefix: YangAtom,
    pub module_name: YangAtom,
    pub keyword: YangAtom,
    pub arg: *mut c_char,
    pub arg_type: *mut std::ffi::c_void,
    pub yang_version: i8,
    pub filename: *mut c_char,
    pub line: c_int,
    pub next: *mut YangStatement,
    pub substmt: *mut YangStatement,
}

static AM_YANG_VERSION: OnceLock<usize> = OnceLock::new();

struct Toks {
    reader: BufReader<File>,
    ectx: *mut YangErrorCtx,
    filename: *mut c_char,
    line: c_int,
    buf: Vec<u8>,
    p: usize,
    expect_eof: bool,
    yang_version: i8,
}

fn is_identifier1(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_identifier2(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-'
}

fn is_space(b: u8) -> bool {
    b == b' '
}

fn is_wspace(b: u8) -> bool {
    is_space(b) || b == b'\t'
}

fn is_wspace_lf(b: u8) -> bool {
    is_wspace(b) || b == b'\n'
}

unsafe fn report_err(toks: &Toks, code: c_int, col: c_int, message: String) {
    let Ok(cmsg) = CString::new(message) else {
        return;
    };
    yang_add_err_msg_rs(
        toks.ectx,
        code,
        toks.filename,
        toks.line,
        col,
        cmsg.as_ptr(),
    );
}

unsafe fn report_err_filename(
    ectx: *mut YangErrorCtx,
    filename: *mut c_char,
    line: c_int,
    col: c_int,
    code: c_int,
    message: String,
) {
    let Ok(cmsg) = CString::new(message) else {
        return;
    };
    yang_add_err_msg_rs(ectx, code, filename, line, col, cmsg.as_ptr());
}

impl Toks {
    fn cur(&self) -> u8 {
        *self.buf.get(self.p).unwrap_or(&0)
    }

    fn at(&self, idx: usize) -> u8 {
        *self.buf.get(idx).unwrap_or(&0)
    }

    fn is_crlf(&self) -> bool {
        self.at(self.p) == b'\r' && self.at(self.p + 1) == b'\n'
    }

    unsafe fn readline(&mut self) -> bool {
        let mut line = Vec::new();
        match self.reader.read_until(b'\n', &mut line) {
            Ok(0) => {
                if !self.expect_eof {
                    report_err(
                        self,
                        YANG_ERR_PARSE_EOF,
                        -1,
                        "premature end of file".to_string(),
                    );
                }
                false
            }
            Ok(_) => {
                line.push(0);
                self.buf = line;
                self.p = 0;
                self.line += 1;
                true
            }
            Err(e) => {
                report_err(self, YANG_ERR_PARSE_EOF, -1, e.to_string());
                false
            }
        }
    }

    unsafe fn skip(&mut self) -> bool {
        loop {
            let c = self.cur();
            if c == 0 {
                if !self.readline() {
                    return false;
                }
            } else if is_wspace_lf(c) || self.is_crlf() {
                self.p += 1;
            } else if c == b'/' {
                let n = self.at(self.p + 1);
                if n == b'/' {
                    if !self.readline() {
                        return false;
                    }
                } else if n == b'*' {
                    self.p += 2;
                    loop {
                        while self.cur() != 0 {
                            if self.cur() == b'*' && self.at(self.p + 1) == b'/' {
                                self.p += 2;
                                break;
                            }
                            self.p += 1;
                        }
                        if self.cur() == 0 {
                            if !self.readline() {
                                return false;
                            }
                            continue;
                        }
                        break;
                    }
                } else {
                    return true;
                }
            } else {
                return true;
            }
        }
    }

    fn looking_at_separator(&self, pos: usize) -> bool {
        let c = self.at(pos);
        if c == 0 {
            return false;
        }
        if is_wspace_lf(c) || c == b';' || c == b'{' || self.is_crlf_at(pos) {
            return true;
        }
        c == b'/' && (self.at(pos + 1) == b'/' || self.at(pos + 1) == b'*')
    }

    fn is_crlf_at(&self, pos: usize) -> bool {
        self.at(pos) == b'\r' && self.at(pos + 1) == b'\n'
    }

    unsafe fn get_keyword(&mut self, prefix: &mut YangAtom, keyword: &mut YangAtom) -> bool {
        let mut p = self.p;
        let mut s = p;
        if !is_identifier1(self.at(p)) {
            report_err(
                self,
                YANG_ERR_PARSE_BAD_KEYWORD,
                1 + (p as c_int),
                format!("invalid keyword start character \"{}\"", self.at(p) as char),
            );
            return false;
        }
        p += 1;
        while is_identifier2(self.at(p)) {
            p += 1;
        }
        if self.at(p) == b':' {
            *prefix =
                yang_make_atom_len(self.buf.as_ptr().add(s) as *const c_char, (p - s) as c_int);
            p += 1;
            s = p;
            if !is_identifier1(self.at(p)) {
                report_err(
                    self,
                    YANG_ERR_PARSE_BAD_KEYWORD,
                    1 + (p as c_int),
                    format!("invalid keyword character \"{}\"", self.at(p) as char),
                );
                return false;
            }
            p += 1;
            while is_identifier2(self.at(p)) {
                p += 1;
            }
        } else {
            *prefix = ptr::null_mut();
        }
        *keyword = yang_make_atom_len(self.buf.as_ptr().add(s) as *const c_char, (p - s) as c_int);
        self.p = p;
        if !self.looking_at_separator(self.p) {
            report_err(
                self,
                YANG_ERR_PARSE_EXPECTED_SEPARATOR,
                1 + (p as c_int),
                "expected token separator".to_string(),
            );
            return false;
        }
        true
    }

    unsafe fn get_string(&mut self, out: &mut *mut c_char) -> bool {
        *out = ptr::null_mut();
        if !self.skip() {
            return false;
        }
        if matches!(self.cur(), b';' | b'{' | b'}') {
            report_err(
                self,
                YANG_ERR_PARSE_EXPECTED_STRING,
                1 + self.p as c_int,
                "expected string".to_string(),
            );
            return false;
        }

        if self.cur() != b'"' && self.cur() != b'\'' {
            let start = self.p;
            while !self.looking_at_separator(self.p)
                && self.cur() != b'}'
                && self.cur() != b'"'
                && self.cur() != b'\''
            {
                self.p += 1;
            }
            let mut v = self.buf[start..self.p].to_vec();
            v.push(0);
            *out = CString::from_vec_with_nul_unchecked(v).into_raw();
            return true;
        }

        let mut result: Vec<u8> = Vec::new();
        loop {
            let quote = self.cur();
            self.p += 1;
            let mut indentpos = self.p;
            for b in &self.buf[..self.p] {
                if *b == b'\t' {
                    indentpos += 7;
                }
            }

            while self.cur() != quote {
                if self.cur() == 0 {
                    if !self.readline() {
                        return false;
                    }
                    if quote == b'"' {
                        let mut i = 0usize;
                        while i < indentpos {
                            match self.cur() {
                                b' ' => {
                                    self.p += 1;
                                    i += 1;
                                }
                                b'\t' => {
                                    self.p += 1;
                                    i += 8;
                                }
                                _ => break,
                            }
                        }
                    }
                    continue;
                }

                if quote == b'\'' {
                    result.push(self.cur());
                    self.p += 1;
                } else if self.cur() == b'\\' {
                    let next = self.at(self.p + 1);
                    match next {
                        b'n' => {
                            result.push(b'\n');
                            self.p += 2;
                        }
                        b't' => {
                            result.push(b'\t');
                            self.p += 2;
                        }
                        b'"' => {
                            result.push(b'"');
                            self.p += 2;
                        }
                        b'\\' => {
                            result.push(b'\\');
                            self.p += 2;
                        }
                        _ => {
                            if self.yang_version == YANG_VERSION_1 {
                                report_err(
                                    self,
                                    YANG_WARN_PARSE_ILLEGAL_ESCAPE,
                                    1 + self.p as c_int,
                                    "illegal character after \\".to_string(),
                                );
                            } else {
                                report_err(
                                    self,
                                    YANG_ERR_PARSE_ILLEGAL_ESCAPE,
                                    1 + self.p as c_int,
                                    "illegal character after \\".to_string(),
                                );
                            }
                            result.push(self.cur());
                            self.p += 1;
                        }
                    }
                } else {
                    result.push(self.cur());
                    self.p += 1;
                }
            }

            self.p += 1;
            if !self.skip() {
                return false;
            }
            if self.cur() == b'+' {
                self.p += 1;
                if !self.skip() {
                    return false;
                }
                if self.cur() == b'"' || self.cur() == b'\'' {
                    continue;
                }
                report_err(
                    self,
                    YANG_ERR_PARSE_EXPECTED_QUOTED_STRING,
                    1 + self.p as c_int,
                    "expected quoted string after '+' operator".to_string(),
                );
                return false;
            }
            break;
        }

        result.push(0);
        *out = CString::from_vec_with_nul_unchecked(result).into_raw();
        true
    }
}

unsafe fn parse_statement(toks: &mut Toks, stmt: &mut *mut YangStatement) -> bool {
    *stmt = ptr::null_mut();
    let mut prefix: YangAtom = ptr::null_mut();
    let mut keyword: YangAtom = ptr::null_mut();
    let mut arg: *mut c_char = ptr::null_mut();

    if !toks.skip() {
        return false;
    }
    let line = toks.line;
    if !toks.get_keyword(&mut prefix, &mut keyword) {
        return false;
    }
    if !toks.skip() {
        return false;
    }
    if toks.cur() != b'{' && toks.cur() != b';' && !toks.get_string(&mut arg) {
        return false;
    }
    if !toks.skip() {
        return false;
    }

    let node = Box::new(YangStatement {
        prefix,
        module_name: ptr::null_mut(),
        keyword,
        arg,
        arg_type: ptr::null_mut(),
        yang_version: 0,
        filename: toks.filename,
        line,
        next: ptr::null_mut(),
        substmt: ptr::null_mut(),
    });
    *stmt = Box::into_raw(node);

    let am = AM_YANG_VERSION.get().copied().unwrap_or(0) as YangAtom;
    if keyword == am && !arg.is_null() {
        let a = CStr::from_ptr(arg).to_bytes();
        if a == b"1.1" {
            toks.yang_version = YANG_VERSION_1_1;
        }
    }

    match toks.cur() {
        b';' => {
            toks.p += 1;
            true
        }
        b'{' => {
            toks.p += 1;
            if !toks.skip() {
                yang_free_tree(*stmt);
                *stmt = ptr::null_mut();
                return false;
            }
            let mut next = &mut (**stmt).substmt as *mut *mut YangStatement;
            while toks.cur() != b'}' {
                let mut tmp: *mut YangStatement = ptr::null_mut();
                if !parse_statement(toks, &mut tmp) {
                    yang_free_tree(*stmt);
                    *stmt = ptr::null_mut();
                    return false;
                }
                *next = tmp;
                next = &mut (*tmp).next;
                if !toks.skip() {
                    yang_free_tree(*stmt);
                    *stmt = ptr::null_mut();
                    return false;
                }
            }
            toks.p += 1;
            true
        }
        _ => {
            let kw = if keyword.is_null() {
                String::new()
            } else {
                CStr::from_ptr(keyword).to_string_lossy().into_owned()
            };
            report_err(
                toks,
                YANG_ERR_PARSE_INCOMPLETE_STATEMENT,
                1 + toks.p as c_int,
                format!("unterminated statement for keyword \"{}\"", kw),
            );
            yang_free_tree(*stmt);
            *stmt = ptr::null_mut();
            false
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn yang_init_parser() -> bool {
    let atom = yang_make_atom(CStr::from_bytes_with_nul_unchecked(b"yang-version\0").as_ptr());
    let _ = AM_YANG_VERSION.set(atom as usize);
    true
}

#[no_mangle]
pub unsafe extern "C" fn yang_free_statement(stmt: *mut YangStatement) {
    if stmt.is_null() {
        return;
    }
    if !(*stmt).arg.is_null() {
        drop(CString::from_raw((*stmt).arg));
    }
    drop(Box::from_raw(stmt));
}

#[no_mangle]
pub unsafe extern "C" fn yang_free_tree(stmt: *mut YangStatement) {
    if stmt.is_null() {
        return;
    }
    let sub = (*stmt).substmt;
    let next = (*stmt).next;
    yang_free_tree(sub);
    yang_free_tree(next);
    yang_free_statement(stmt);
}

#[no_mangle]
pub unsafe extern "C" fn yang_parse(
    filename: *mut c_char,
    stmt: *mut *mut YangStatement,
    ectx: *mut YangErrorCtx,
) -> bool {
    if filename.is_null() || stmt.is_null() || ectx.is_null() {
        return false;
    }

    let path_bytes = CStr::from_ptr(filename).to_bytes();
    let path = OsStr::from_bytes(path_bytes);
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            report_err_filename(ectx, filename, 0, -1, YANG_ERR_PARSE_FOPEN, e.to_string());
            return false;
        }
    };

    let mut toks = Toks {
        reader: BufReader::new(file),
        ectx,
        filename,
        line: 0,
        buf: vec![0],
        p: 0,
        expect_eof: false,
        yang_version: YANG_VERSION_1,
    };

    if !toks.readline() {
        return false;
    }

    let ok = parse_statement(&mut toks, &mut *stmt);
    toks.expect_eof = true;
    if ok && toks.skip() {
        report_err(
            &toks,
            YANG_ERR_PARSE_TRAILING_GARBAGE,
            1 + toks.p as c_int,
            "trailing garbage after module".to_string(),
        );
    }
    ok
}
