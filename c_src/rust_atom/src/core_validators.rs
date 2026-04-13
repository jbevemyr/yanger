use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};

const YANG_VERSION_1: i8 = 1;

#[repr(C)]
struct XmlRegexp {
    _private: [u8; 0],
}

extern "C" {
    fn xmlRegexpExec(comp: *mut XmlRegexp, content: *const u8) -> c_int;
    fn yang_parse_if_feature_expr(str_ptr: *mut c_char) -> bool;
}

unsafe fn cstr(ptr: *mut c_char) -> Option<&'static CStr> {
    if ptr.is_null() {
        None
    } else {
        Some(CStr::from_ptr(ptr))
    }
}

unsafe fn write_err(errbuf: *mut c_char, sz: c_int, msg: &str) {
    if errbuf.is_null() || sz <= 0 {
        return;
    }
    let Ok(cmsg) = CString::new(msg) else {
        *errbuf = 0;
        return;
    };
    let bytes = cmsg.as_bytes_with_nul();
    let n = usize::min(bytes.len(), sz as usize);
    std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, errbuf, n);
    if n == sz as usize {
        *errbuf.add((n - 1) as usize) = 0;
    }
}

fn starts_with_ws(s: &str) -> bool {
    s.chars().next().map(|c| c.is_whitespace()).unwrap_or(false)
}

#[no_mangle]
pub unsafe extern "C" fn chk_enum_arg_rs(
    arg: *mut c_char,
    _opaque: *mut c_void,
    _yang_version: c_char,
    _errbuf: *mut c_char,
    _sz: c_int,
) -> bool {
    let Some(arg) = cstr(arg) else {
        return false;
    };
    let s = arg.to_string_lossy();
    if s.is_empty() {
        return false;
    }
    !starts_with_ws(&s) && !s.chars().last().map(|c| c.is_whitespace()).unwrap_or(false)
}

fn parse_i64_str(s: &str) -> Option<i64> {
    if starts_with_ws(s) {
        return None;
    }
    s.parse::<i64>().ok()
}

fn parse_u64_str(s: &str) -> Option<u64> {
    if starts_with_ws(s) || s.starts_with('-') {
        return None;
    }
    s.parse::<u64>().ok()
}

#[no_mangle]
pub unsafe extern "C" fn chk_integer_rs(
    arg: *mut c_char,
    _opaque: *mut c_void,
    _yang_version: c_char,
    _errbuf: *mut c_char,
    _sz: c_int,
) -> bool {
    let Some(arg) = cstr(arg) else {
        return false;
    };
    parse_i64_str(&arg.to_string_lossy()).is_some()
}

#[no_mangle]
pub unsafe extern "C" fn chk_non_negative_integer_rs(
    arg: *mut c_char,
    _opaque: *mut c_void,
    _yang_version: c_char,
    _errbuf: *mut c_char,
    _sz: c_int,
) -> bool {
    let Some(arg) = cstr(arg) else {
        return false;
    };
    parse_u64_str(&arg.to_string_lossy()).is_some()
}

#[no_mangle]
pub unsafe extern "C" fn chk_max_value_rs(
    arg: *mut c_char,
    _opaque: *mut c_void,
    _yang_version: c_char,
    _errbuf: *mut c_char,
    _sz: c_int,
) -> bool {
    let Some(arg) = cstr(arg) else {
        return false;
    };
    let s = arg.to_string_lossy();
    if s == "unbounded" {
        true
    } else {
        parse_u64_str(&s).is_some()
    }
}

#[no_mangle]
pub unsafe extern "C" fn chk_fraction_digits_arg_rs(
    arg: *mut c_char,
    _opaque: *mut c_void,
    _yang_version: c_char,
    _errbuf: *mut c_char,
    _sz: c_int,
) -> bool {
    let Some(arg) = cstr(arg) else {
        return false;
    };
    match parse_u64_str(&arg.to_string_lossy()) {
        Some(v) => (1..=18).contains(&v),
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn chk_identifier_rs(
    arg: *mut c_char,
    opaque: *mut c_void,
    yang_version: c_char,
    errbuf: *mut c_char,
    sz: c_int,
) -> bool {
    if arg.is_null() || opaque.is_null() {
        return false;
    }
    if xmlRegexpExec(opaque as *mut XmlRegexp, arg as *const u8) != 1 {
        return false;
    }
    if yang_version != YANG_VERSION_1 as c_char {
        return true;
    }
    let s = CStr::from_ptr(arg).to_string_lossy();
    if s.len() >= 3 && s[..3].eq_ignore_ascii_case("xml") {
        write_err(
            errbuf,
            sz,
            &format!(
                "bad argument value \"{}\", an identifier must not start with [xX][mM][lL] in YANG version 1",
                s
            ),
        );
        return false;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn chk_date_rs(
    arg: *mut c_char,
    opaque: *mut c_void,
    _yang_version: c_char,
    _errbuf: *mut c_char,
    _sz: c_int,
) -> bool {
    if arg.is_null() || opaque.is_null() {
        return false;
    }
    if xmlRegexpExec(opaque as *mut XmlRegexp, arg as *const u8) != 1 {
        return false;
    }
    let s = CStr::from_ptr(arg).to_string_lossy();
    let parts: Vec<_> = s.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    let (Ok(y), Ok(m), Ok(d)) = (
        parts[0].parse::<i32>(),
        parts[1].parse::<i32>(),
        parts[2].parse::<i32>(),
    ) else {
        return false;
    };
    if !(1..=12).contains(&m) || d < 1 {
        return false;
    }
    let mut days = 31;
    if m == 2 {
        days = if y % 400 == 0 || (y % 4 == 0 && y % 100 != 0) {
            29
        } else {
            28
        };
    } else if matches!(m, 4 | 6 | 9 | 11) {
        days = 30;
    }
    d <= days
}

#[no_mangle]
pub unsafe extern "C" fn chk_if_feature_expr_rs(
    arg: *mut c_char,
    opaque: *mut c_void,
    yang_version: c_char,
    errbuf: *mut c_char,
    sz: c_int,
) -> bool {
    if arg.is_null() {
        return false;
    }
    if yang_version == YANG_VERSION_1 as c_char {
        if xmlRegexpExec(opaque as *mut XmlRegexp, arg as *const u8) != 1 {
            let s = CStr::from_ptr(arg).to_string_lossy();
            write_err(
                errbuf,
                sz,
                &format!(
                    "bad argument value \"{}\", should be of type identifier-ref in YANG version 1",
                    s
                ),
            );
            return false;
        }
        true
    } else {
        yang_parse_if_feature_expr(arg)
    }
}
