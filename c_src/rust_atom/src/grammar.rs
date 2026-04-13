use crate::core_types::init_core_stmt_types;
use crate::parser::{yang_init_parser, YangStatement};
use crate::{yang_add_err_msg_rs, yang_make_atom, YangErrorCtx};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::OnceLock;

const YANG_VERSION_1: i8 = 1;
const YANG_VERSION_1_1: i8 = 2;

const F_STMT_ARG_MATCH: u32 = 1 << 0;

const F_ARG_TYPE_SYNTAX_REGEXP: u32 = 1 << 0;
const F_ARG_TYPE_SYNTAX_CB: u32 = 1 << 1;
const F_ERL_ATOM: u32 = 1 << 16;
const F_ERL_INT: u32 = 1 << 17;
const F_ERL_UINT: u32 = 1 << 18;
const F_ERL_IDENTIFIER_REF: u32 = 1 << 19;
const F_ERL_ATOM_OR_UINT: u32 = 1 << 21;

const YANG_ERR_INTERNAL: c_int = 1;
const YANG_ERR_GRAMMAR_KEYWORD_ALREADY_FOUND: c_int = 200;
const YANG_ERR_GRAMMAR_EXPECTED_KEYWORD: c_int = 201;
const YANG_ERR_GRAMMAR_UNEXPECTED_KEYWORD: c_int = 202;
const YANG_ERR_GRAMMAR_UNDEFINED_PREFIX: c_int = 203;
const YANG_ERR_GRAMMAR_DUPLICATE_PREFIX: c_int = 204;
const YANG_ERR_GRAMMAR_MISSING_ARGUMENT: c_int = 205;
const YANG_ERR_GRAMMAR_UNEXPECTED_ARGUMENT: c_int = 206;
const YANG_ERR_GRAMMAR_BAD_ARGUMENT: c_int = 207;

type YangAtom = *mut c_char;
type ValidateFn =
    Option<unsafe extern "C" fn(*mut c_char, *mut c_void, c_char, *mut c_char, c_int) -> bool>;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct YangArgTypeCb {
    pub(crate) validate: ValidateFn,
    pub(crate) opaque: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union YangArgTypeSyntax {
    pub(crate) xsd_regexp: *mut c_char,
    pub(crate) cb: YangArgTypeCb,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct YangArgType {
    pub(crate) name: YangAtom,
    pub(crate) syntax: YangArgTypeSyntax,
    pub(crate) flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct YangStatementRule {
    module_name: YangAtom,
    keyword: YangAtom,
    min_yang_version: c_char,
    spec: *mut YangStatementSpec,
    occurance: c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct YangStatementSpec {
    keyword: YangAtom,
    arg_type_idx: c_int,
    flags: u32,
    rules: *mut YangStatementRule,
    nrules: c_int,
}

#[repr(C)]
struct Grammar {
    module_name: YangAtom,
    specs: *mut YangStatementSpec,
    nspecs: c_int,
}

#[repr(C)]
struct PrefixMap {
    prefix: YangAtom,
    module_name: YangAtom,
    filename: *mut c_char,
    line: c_int,
}

#[repr(C)]
struct XmlRegexp {
    _private: [u8; 0],
}

extern "C" {
    fn xmlRegexpCompile(regexp: *const u8) -> *mut XmlRegexp;
    fn xmlRegexpExec(comp: *mut XmlRegexp, content: *const u8) -> c_int;
}

static mut AM_MODULE: YangAtom = ptr::null_mut();
static mut AM_SUBMODULE: YangAtom = ptr::null_mut();
static mut AM_YANG_VERSION: YangAtom = ptr::null_mut();
static mut AM_NAMESPACE: YangAtom = ptr::null_mut();
static mut AM_PREFIX: YangAtom = ptr::null_mut();
static mut AM_IMPORT: YangAtom = ptr::null_mut();
static mut AM_INCLUDE: YangAtom = ptr::null_mut();
static mut AM_BELONGS_TO: YangAtom = ptr::null_mut();
static mut AM_SP_CUT: YangAtom = ptr::null_mut();

static mut GRAMMAR: *mut Grammar = ptr::null_mut();
static mut NGRAMMAR: c_int = 0;
static mut TYPES: *mut YangArgType = ptr::null_mut();
static mut NTYPES: c_int = 0;
static CORE_STMTS_OWNED: OnceLock<CoreStmtsOwned> = OnceLock::new();

include!(concat!(env!("OUT_DIR"), "/core_stmts_generated.rs"));

struct CoreStmtsOwned {
    _strings: Vec<CString>,
    table_addrs: Vec<usize>,
}

fn core_stmts_table_ptr() -> *const *const c_char {
    if CORE_STMTS.is_empty() {
        return ptr::null();
    }

    CORE_STMTS_OWNED
        .get_or_init(|| {
            let mut strings: Vec<CString> = Vec::new();
            let mut table_addrs: Vec<usize> = Vec::new();
            for entry in CORE_STMTS {
                match entry {
                    Some(s) => {
                        let cs = CString::new(*s).expect("core statement contains interior NUL");
                        let p = cs.as_ptr();
                        strings.push(cs);
                        table_addrs.push(p as usize);
                    }
                    None => table_addrs.push(ptr::null::<c_char>() as usize),
                }
            }
            CoreStmtsOwned {
                _strings: strings,
                table_addrs,
            }
        })
        .table_addrs
        .as_ptr() as *const *const c_char
}

unsafe fn cstr_to_string(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    CStr::from_ptr(p).to_string_lossy().into_owned()
}

fn add_gen_err(
    ectx: *mut YangErrorCtx,
    filename: *mut c_char,
    line: c_int,
    col: c_int,
    code: c_int,
    msg: String,
) {
    let Ok(cmsg) = CString::new(msg) else {
        return;
    };
    unsafe { yang_add_err_msg_rs(ectx, code, filename, line, col, cmsg.as_ptr()) };
}

fn add_stmt_err(ectx: *mut YangErrorCtx, stmt: *mut YangStatement, code: c_int, msg: String) {
    if stmt.is_null() {
        return;
    }
    unsafe { add_gen_err(ectx, (*stmt).filename, (*stmt).line, -1, code, msg) };
}

fn fmt_vsn(vsn: c_char) -> &'static str {
    if vsn == YANG_VERSION_1 as c_char {
        "1"
    } else if vsn == YANG_VERSION_1_1 as c_char {
        "1.1"
    } else {
        "unknown version"
    }
}

fn get_grammar(module_name: YangAtom) -> *mut Grammar {
    unsafe {
        let mut i = 0;
        while i < NGRAMMAR {
            let g = GRAMMAR.add(i as usize);
            if (*g).module_name == module_name {
                return g;
            }
            i += 1;
        }
        ptr::null_mut()
    }
}

fn build_keyword_from_stmt(stmt: *mut YangStatement) -> String {
    if stmt.is_null() {
        return String::new();
    }
    let mut out = String::new();
    unsafe {
        if !(*stmt).prefix.is_null() {
            out.push_str(&cstr_to_string((*stmt).prefix));
            out.push(':');
        }
        out.push_str(&cstr_to_string((*stmt).keyword));
    }
    out
}

fn build_keyword_from_rule(rule: *mut YangStatementRule) -> String {
    if rule.is_null() {
        return String::new();
    }
    let mut out = String::new();
    unsafe {
        if !(*rule).module_name.is_null() {
            out.push_str(&cstr_to_string((*rule).module_name));
            out.push(':');
        }
        out.push_str(&cstr_to_string((*rule).keyword));
    }
    out
}

fn get_spec_from_rule(
    g: *mut Grammar,
    rule: *mut YangStatementRule,
    mut offset: c_int,
) -> *mut YangStatementSpec {
    if g.is_null() || rule.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let mut i = 0;
        while i < (*g).nspecs {
            let s = (*g).specs.add(i as usize);
            if (*rule).keyword == (*s).keyword {
                if offset > 0 {
                    offset -= 1;
                } else {
                    return s;
                }
            }
            i += 1;
        }
        ptr::null_mut()
    }
}

unsafe extern "C" fn chk_xsd_regexp(
    arg: *mut c_char,
    opaque: *mut c_void,
    _yang_version: c_char,
    _errbuf: *mut c_char,
    _sz: c_int,
) -> bool {
    if arg.is_null() || opaque.is_null() {
        return false;
    }
    xmlRegexpExec(opaque as *mut XmlRegexp, arg as *const u8) == 1
}

unsafe fn fix_grammar() -> bool {
    let mut i = 0;
    while i < NGRAMMAR {
        let g = GRAMMAR.add(i as usize);
        let mut j = 0;
        while j < (*g).nspecs {
            let s = (*g).specs.add(j as usize);
            let mut offset = 0;
            let mut k = 0;
            while k < (*s).nrules {
                let r = (*s).rules.add(k as usize);
                if k > 0 {
                    let prev = (*s).rules.add((k - 1) as usize);
                    if (*prev).keyword == (*r).keyword {
                        offset += 1;
                    } else {
                        offset = 0;
                    }
                } else {
                    offset = 0;
                }
                if (*r).spec.is_null() && (*r).keyword != AM_SP_CUT {
                    let rg = get_grammar((*r).module_name);
                    if rg.is_null() {
                        return false;
                    }
                    let spec = get_spec_from_rule(rg, r, offset);
                    if spec.is_null() {
                        return false;
                    }
                    (*r).spec = spec;
                }
                k += 1;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn yang_get_grammar_module_names(
    n: c_int,
    module_names: *mut YangAtom,
) -> c_int {
    if n < NGRAMMAR - 1 {
        return NGRAMMAR - 1;
    }
    let mut i = 0;
    let mut j = 0;
    while i < NGRAMMAR {
        let g = GRAMMAR.add(i as usize);
        if !(*g).module_name.is_null() {
            if !module_names.is_null() {
                *module_names.add(j as usize) = (*g).module_name;
            }
            j += 1;
        }
        i += 1;
    }
    j
}

#[no_mangle]
pub unsafe extern "C" fn yang_get_statement_spec(
    module_name: YangAtom,
    keyword: YangAtom,
) -> *mut YangStatementSpec {
    let g = get_grammar(module_name);
    if g.is_null() {
        return ptr::null_mut();
    }
    let mut i = 0;
    while i < (*g).nspecs {
        let s = (*g).specs.add(i as usize);
        if (*s).keyword == keyword && ((*s).flags & F_STMT_ARG_MATCH) == 0 {
            return s;
        }
        i += 1;
    }
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn yang_get_arg_type_idx(name: YangAtom) -> c_int {
    let mut i = 0;
    while i < NTYPES {
        if (*TYPES.add(i as usize)).name == name {
            return i;
        }
        i += 1;
    }
    -1
}

#[no_mangle]
pub unsafe extern "C" fn yang_get_arg_type(arg_type_idx: c_int) -> *mut YangArgType {
    if arg_type_idx >= 0 && arg_type_idx < NTYPES {
        TYPES.add(arg_type_idx as usize)
    } else {
        ptr::null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn yang_install_arg_types(new_types: *mut YangArgType, len: c_int) -> bool {
    let old = NTYPES;
    NTYPES += len;
    TYPES = libc::realloc(
        TYPES as *mut c_void,
        (NTYPES as usize) * std::mem::size_of::<YangArgType>(),
    ) as *mut YangArgType;
    if TYPES.is_null() {
        return false;
    }
    let mut j = 0;
    let mut i = old;
    while j < len {
        let src = *new_types.add(j as usize);
        *TYPES.add(i as usize) = src;
        if (src.flags & F_ARG_TYPE_SYNTAX_REGEXP) != 0 {
            let xreg = xmlRegexpCompile(src.syntax.xsd_regexp as *const u8);
            if xreg.is_null() {
                return false;
            }
            let t = TYPES.add(i as usize);
            (*t).syntax = YangArgTypeSyntax {
                cb: YangArgTypeCb {
                    validate: Some(chk_xsd_regexp),
                    opaque: xreg as *mut c_void,
                },
            };
            (*t).flags &= !F_ARG_TYPE_SYNTAX_REGEXP;
            (*t).flags |= F_ARG_TYPE_SYNTAX_CB;
        }
        i += 1;
        j += 1;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn yang_install_grammar(
    module_name: YangAtom,
    new_specs: *mut YangStatementSpec,
    len: c_int,
) -> bool {
    if !get_grammar(module_name).is_null() {
        return false;
    }
    let start = NGRAMMAR;
    NGRAMMAR += 1;
    GRAMMAR = libc::realloc(
        GRAMMAR as *mut c_void,
        (NGRAMMAR as usize) * std::mem::size_of::<Grammar>(),
    ) as *mut Grammar;
    if GRAMMAR.is_null() {
        return false;
    }
    let g = GRAMMAR.add(start as usize);
    (*g).module_name = module_name;
    (*g).nspecs = len;
    (*g).specs = libc::malloc((len as usize) * std::mem::size_of::<YangStatementSpec>())
        as *mut YangStatementSpec;
    if (*g).specs.is_null() {
        return false;
    }

    let mut i = 0;
    while i < len {
        let src = *new_specs.add(i as usize);
        let dst = (*g).specs.add(i as usize);
        (*dst).keyword = src.keyword;
        (*dst).arg_type_idx = src.arg_type_idx;
        (*dst).flags = src.flags;
        (*dst).nrules = src.nrules;
        (*dst).rules =
            libc::malloc((src.nrules as usize) * std::mem::size_of::<YangStatementRule>())
                as *mut YangStatementRule;
        if (*dst).rules.is_null() {
            return false;
        }
        let mut j = 0;
        while j < src.nrules {
            *(*dst).rules.add(j as usize) = *src.rules.add(j as usize);
            (*(*dst).rules.add(j as usize)).spec = ptr::null_mut();
            j += 1;
        }
        i += 1;
    }
    fix_grammar()
}

#[no_mangle]
pub unsafe extern "C" fn yang_add_rule_to_spec(
    rule: *mut YangStatementRule,
    modulename: YangAtom,
    keyword: YangAtom,
) -> bool {
    let s = yang_get_statement_spec(modulename, keyword);
    if s.is_null() || rule.is_null() {
        return false;
    }
    (*s).nrules += 1;
    (*s).rules = libc::realloc(
        (*s).rules as *mut c_void,
        (*s).nrules as usize * std::mem::size_of::<YangStatementRule>(),
    ) as *mut YangStatementRule;
    if (*s).rules.is_null() {
        return false;
    }
    *(*s).rules.add(((*s).nrules - 1) as usize) = *rule;
    true
}

unsafe fn set_module_name_from_prefix(
    stmt: *mut YangStatement,
    prefix_map: *mut PrefixMap,
    nprefixes: c_int,
    vsn: c_char,
    ectx: *mut YangErrorCtx,
) {
    if stmt.is_null() {
        return;
    }
    (*stmt).yang_version = vsn;
    if !(*stmt).prefix.is_null() {
        let mut i = 0;
        while i < nprefixes {
            let pm = prefix_map.add(i as usize);
            if (*stmt).prefix == (*pm).prefix {
                (*stmt).module_name = (*pm).module_name;
                break;
            }
            i += 1;
        }
        if i == nprefixes {
            add_stmt_err(
                ectx,
                stmt,
                YANG_ERR_GRAMMAR_UNDEFINED_PREFIX,
                format!("undefined prefix {}", cstr_to_string((*stmt).prefix)),
            );
        }
    }
    set_module_name_from_prefix((*stmt).substmt, prefix_map, nprefixes, vsn, ectx);
    set_module_name_from_prefix((*stmt).next, prefix_map, nprefixes, vsn, ectx);
}

unsafe fn add_prefix(
    stmt: *mut YangStatement,
    module_name: YangAtom,
    prefix_map: *mut PrefixMap,
    n: &mut c_int,
    ectx: *mut YangErrorCtx,
) {
    if stmt.is_null() || (*stmt).arg.is_null() {
        return;
    }
    let prefix = yang_make_atom((*stmt).arg);
    let mut i = 0;
    while i < *n {
        let pm = prefix_map.add(i as usize);
        if prefix == (*pm).prefix {
            add_stmt_err(
                ectx,
                stmt,
                YANG_ERR_GRAMMAR_DUPLICATE_PREFIX,
                format!(
                    "prefix '{}' already defined at {}:{}",
                    cstr_to_string(prefix),
                    cstr_to_string((*pm).filename),
                    (*pm).line
                ),
            );
            return;
        }
        i += 1;
    }
    let pm = prefix_map.add(*n as usize);
    (*pm).prefix = prefix;
    (*pm).module_name = module_name;
    (*pm).filename = (*stmt).filename;
    (*pm).line = (*stmt).line;
    *n += 1;
}

unsafe fn resolve_module_names_from_prefixes(
    nprefixes: c_int,
    stmt: *mut YangStatement,
    ectx: *mut YangErrorCtx,
) {
    if stmt.is_null() || nprefixes <= 0 {
        return;
    }
    let prefix_map =
        libc::calloc(nprefixes as usize, std::mem::size_of::<PrefixMap>()) as *mut PrefixMap;
    if prefix_map.is_null() {
        return;
    }
    let mut n = 0;
    let mut vsn = YANG_VERSION_1 as c_char;
    let mut s = (*stmt).substmt;
    while !s.is_null() && n < nprefixes {
        if (*s).prefix.is_null() {
            if (*s).keyword == AM_PREFIX && !(*stmt).arg.is_null() {
                add_prefix(s, yang_make_atom((*stmt).arg), prefix_map, &mut n, ectx);
            } else if (*s).keyword == AM_BELONGS_TO && !(*s).arg.is_null() {
                let mut s2 = (*s).substmt;
                while !s2.is_null() {
                    if (*s2).prefix.is_null() && (*s2).keyword == AM_PREFIX {
                        add_prefix(s2, yang_make_atom((*s).arg), prefix_map, &mut n, ectx);
                        break;
                    }
                    s2 = (*s2).next;
                }
            } else if (*s).keyword == AM_IMPORT && !(*s).arg.is_null() {
                let mut s2 = (*s).substmt;
                while !s2.is_null() {
                    if (*s2).prefix.is_null() && (*s2).keyword == AM_PREFIX {
                        add_prefix(s2, yang_make_atom((*s).arg), prefix_map, &mut n, ectx);
                        break;
                    }
                    s2 = (*s2).next;
                }
            }
        }
        s = (*s).next;
    }

    s = (*stmt).substmt;
    while !s.is_null() {
        if (*s).keyword == AM_YANG_VERSION {
            if !(*s).arg.is_null() && CStr::from_ptr((*s).arg).to_bytes() == b"1.1" {
                vsn = YANG_VERSION_1_1 as c_char;
            }
            break;
        }
        if (*s).prefix.is_null()
            && (((*stmt).keyword == AM_MODULE
                && (*s).keyword != AM_NAMESPACE
                && (*s).keyword != AM_PREFIX)
                || ((*stmt).keyword == AM_SUBMODULE && (*s).keyword != AM_BELONGS_TO))
        {
            break;
        }
        s = (*s).next;
    }
    (*stmt).yang_version = vsn;
    set_module_name_from_prefix(stmt, prefix_map, nprefixes, vsn, ectx);
    libc::free(prefix_map as *mut c_void);
}

unsafe fn match_rule(
    stmt: *mut YangStatement,
    rules: *mut YangStatementRule,
    start: &mut c_int,
    nrules: c_int,
    found: &mut *mut YangStatementRule,
    mut canonical: bool,
    ectx: *mut YangErrorCtx,
) -> bool {
    let mut i = *start;
    while i < nrules {
        let r = rules.add(i as usize);
        let mut rule_match = false;
        if (*stmt).module_name == (*r).module_name && (*stmt).keyword == (*r).keyword {
            if !(*r).spec.is_null() && ((*(*r).spec).flags & F_STMT_ARG_MATCH) != 0 {
                let t = TYPES.add((*(*r).spec).arg_type_idx as usize);
                let cb = (*t).syntax.cb;
                if let Some(validate) = cb.validate {
                    rule_match = validate(
                        (*stmt).arg,
                        cb.opaque,
                        (*stmt).yang_version,
                        ptr::null_mut(),
                        0,
                    );
                }
            } else {
                rule_match = true;
            }
        }
        if rule_match {
            if (*r).min_yang_version > (*stmt).yang_version {
                add_stmt_err(
                    ectx,
                    stmt,
                    YANG_ERR_GRAMMAR_UNEXPECTED_KEYWORD,
                    format!(
                        "{} not valid in YANG version {}",
                        build_keyword_from_stmt(stmt),
                        fmt_vsn((*stmt).yang_version)
                    ),
                );
            }
            match (*r).occurance as u8 as char {
                '1' | '?' => {
                    (*r).occurance = b'0' as c_char;
                    *found = r;
                    return true;
                }
                '*' => {
                    *found = r;
                    return true;
                }
                '+' => {
                    (*r).occurance = b'*' as c_char;
                    *found = r;
                    return true;
                }
                '0' => {
                    add_stmt_err(
                        ectx,
                        stmt,
                        YANG_ERR_GRAMMAR_KEYWORD_ALREADY_FOUND,
                        format!("keyword '{}' already given", build_keyword_from_stmt(stmt)),
                    );
                    return false;
                }
                '-' => {
                    add_stmt_err(
                        ectx,
                        stmt,
                        YANG_ERR_GRAMMAR_UNEXPECTED_KEYWORD,
                        format!("unexpected keyword '{}'", build_keyword_from_stmt(stmt)),
                    );
                    return false;
                }
                _ => {}
            }
        } else if !(*stmt).prefix.is_null() {
            canonical = false;
        } else if (*r).keyword == AM_SP_CUT {
            let mut j = *start;
            while j < i {
                let rj = rules.add(j as usize);
                let occ = (*rj).occurance as u8 as char;
                if occ == '1' || occ == '+' {
                    (*rj).occurance = b'0' as c_char;
                    add_stmt_err(
                        ectx,
                        stmt,
                        YANG_ERR_GRAMMAR_EXPECTED_KEYWORD,
                        format!("expected keyword '{}'", build_keyword_from_rule(rj)),
                    );
                    return false;
                }
                j += 1;
            }
            *start = i + 1;
        } else if canonical {
            let occ = (*r).occurance as u8 as char;
            if occ == '1' || occ == '+' {
                (*r).occurance = b'0' as c_char;
                add_stmt_err(
                    ectx,
                    stmt,
                    YANG_ERR_GRAMMAR_EXPECTED_KEYWORD,
                    format!(
                        "expected keyword '{}' before '{}'",
                        build_keyword_from_rule(r),
                        build_keyword_from_stmt(stmt)
                    ),
                );
                return false;
            } else {
                (*r).occurance = b'-' as c_char;
            }
        }
        i += 1;
    }
    add_stmt_err(
        ectx,
        stmt,
        YANG_ERR_GRAMMAR_UNEXPECTED_KEYWORD,
        format!("unexpected keyword '{}'", build_keyword_from_stmt(stmt)),
    );
    false
}

unsafe fn chk_statements(
    mut stmt: *mut YangStatement,
    parent: *mut YangStatement,
    rules: *mut YangStatementRule,
    nrules: c_int,
    canonical: bool,
    ectx: *mut YangErrorCtx,
    res: &mut bool,
) {
    let mut start = 0;
    while !stmt.is_null() {
        let g = if (*stmt).module_name.is_null() && !(*stmt).prefix.is_null() {
            ptr::null_mut()
        } else {
            get_grammar((*stmt).module_name)
        };
        if !g.is_null() {
            let mut rule: *mut YangStatementRule = ptr::null_mut();
            if !match_rule(stmt, rules, &mut start, nrules, &mut rule, canonical, ectx) {
                *res = false;
                stmt = (*stmt).next;
                continue;
            }
            let subspec = (*rule).spec;
            if (*subspec).arg_type_idx != -1 && !(*stmt).arg.is_null() {
                (*stmt).arg_type = yang_get_arg_type((*subspec).arg_type_idx) as *mut c_void;
                let at = (*stmt).arg_type as *mut YangArgType;
                if ((*at).flags & F_ARG_TYPE_SYNTAX_CB) != 0 {
                    let mut errbuf = [0i8; libc::BUFSIZ as usize];
                    let cb = (*at).syntax.cb;
                    if let Some(validate) = cb.validate {
                        if !validate(
                            (*stmt).arg,
                            cb.opaque,
                            (*stmt).yang_version,
                            errbuf.as_mut_ptr(),
                            errbuf.len() as c_int,
                        ) {
                            *res = false;
                            if errbuf[0] == 0 {
                                add_stmt_err(
                                    ectx,
                                    stmt,
                                    YANG_ERR_GRAMMAR_BAD_ARGUMENT,
                                    format!(
                                        "bad argument value \"{}\", should be of type {}",
                                        cstr_to_string((*stmt).arg),
                                        cstr_to_string((*at).name)
                                    ),
                                );
                            } else {
                                add_stmt_err(
                                    ectx,
                                    stmt,
                                    YANG_ERR_GRAMMAR_BAD_ARGUMENT,
                                    cstr_to_string(errbuf.as_ptr()),
                                );
                            }
                        }
                    }
                }
            } else if (*subspec).arg_type_idx != -1 && (*stmt).arg.is_null() {
                *res = false;
                add_stmt_err(
                    ectx,
                    stmt,
                    YANG_ERR_GRAMMAR_MISSING_ARGUMENT,
                    format!("missing argument to '{}'", build_keyword_from_rule(rule)),
                );
            } else if (*subspec).arg_type_idx == -1 && !(*stmt).arg.is_null() {
                *res = false;
                add_stmt_err(
                    ectx,
                    stmt,
                    YANG_ERR_GRAMMAR_UNEXPECTED_ARGUMENT,
                    format!(
                        "did not expect an argument to '{}', got \"{}\"",
                        build_keyword_from_rule(rule),
                        cstr_to_string((*stmt).arg)
                    ),
                );
            }

            let sz = (*subspec).nrules as usize * std::mem::size_of::<YangStatementRule>();
            let subrules = libc::malloc(sz) as *mut YangStatementRule;
            if subrules.is_null() {
                *res = false;
                return;
            }
            ptr::copy_nonoverlapping((*subspec).rules, subrules, (*subspec).nrules as usize);
            chk_statements(
                (*stmt).substmt,
                stmt,
                subrules,
                (*subspec).nrules,
                canonical,
                ectx,
                res,
            );
            libc::free(subrules as *mut c_void);
        }
        stmt = (*stmt).next;
    }
    let mut i = 0;
    while i < nrules {
        let r = rules.add(i as usize);
        let occ = (*r).occurance as u8 as char;
        if occ == '1' || occ == '+' {
            *res = false;
            add_stmt_err(
                ectx,
                parent,
                YANG_ERR_GRAMMAR_EXPECTED_KEYWORD,
                format!(
                    "expected keyword '{}' as substatement to '{}'",
                    build_keyword_from_rule(r),
                    build_keyword_from_stmt(parent)
                ),
            );
        }
        i += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn yang_grammar_check_module(
    stmt: *mut YangStatement,
    canonical: bool,
    ectx: *mut YangErrorCtx,
) -> bool {
    if stmt.is_null() {
        return false;
    }
    let mut top_rule = YangStatementRule {
        module_name: ptr::null_mut(),
        keyword: ptr::null_mut(),
        min_yang_version: YANG_VERSION_1 as c_char,
        spec: ptr::null_mut(),
        occurance: b'1' as c_char,
    };
    if (*stmt).keyword == AM_MODULE {
        top_rule.keyword = AM_MODULE;
    } else if (*stmt).keyword == AM_SUBMODULE {
        top_rule.keyword = AM_SUBMODULE;
    } else {
        add_stmt_err(
            ectx,
            stmt,
            YANG_ERR_GRAMMAR_UNEXPECTED_KEYWORD,
            format!("unexpected keyword '{}'", build_keyword_from_stmt(stmt)),
        );
        return false;
    }
    top_rule.spec = get_spec_from_rule(get_grammar(ptr::null_mut()), &mut top_rule, 0);
    if top_rule.spec.is_null() {
        add_stmt_err(
            ectx,
            stmt,
            YANG_ERR_INTERNAL,
            "top-level grammar rule not found".to_string(),
        );
        return false;
    }

    let mut nprefixes = 1;
    let mut tmp = (*stmt).substmt;
    while !tmp.is_null() {
        if (*tmp).prefix.is_null() {
            if (*tmp).keyword == AM_IMPORT {
                nprefixes += 1;
            } else if (*tmp).keyword != AM_YANG_VERSION
                && (*tmp).keyword != AM_NAMESPACE
                && (*tmp).keyword != AM_PREFIX
                && (*tmp).keyword != AM_BELONGS_TO
                && (*tmp).keyword != AM_INCLUDE
            {
                break;
            }
        }
        tmp = (*tmp).next;
    }
    resolve_module_names_from_prefixes(nprefixes, stmt, ectx);
    let mut res = true;
    chk_statements(
        stmt,
        ptr::null_mut(),
        &mut top_rule,
        1,
        canonical,
        ectx,
        &mut res,
    );
    res
}

#[no_mangle]
pub unsafe extern "C" fn yang_init_grammar() -> bool {
    AM_MODULE = yang_make_atom(c"module".as_ptr());
    AM_SUBMODULE = yang_make_atom(c"submodule".as_ptr());
    AM_YANG_VERSION = yang_make_atom(c"yang-version".as_ptr());
    AM_NAMESPACE = yang_make_atom(c"namespace".as_ptr());
    AM_PREFIX = yang_make_atom(c"prefix".as_ptr());
    AM_IMPORT = yang_make_atom(c"import".as_ptr());
    AM_INCLUDE = yang_make_atom(c"include".as_ptr());
    AM_BELONGS_TO = yang_make_atom(c"belongs-to".as_ptr());
    AM_SP_CUT = yang_make_atom(c"$cut".as_ptr());

    if yang_init_core_stmt_grammar() == 0 {
        return false;
    }
    yang_init_parser()
}

unsafe fn yang_init_core_stmt_grammar() -> c_int {
    if init_core_stmt_types() == 0 {
        return 0;
    }
    let stmts = core_stmts_table_ptr();
    if stmts.is_null() || !yang_install_grammar_str(ptr::null(), stmts) {
        return 0;
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn yang_print_grammar() {
    let mut g = 0;
    while g < NGRAMMAR {
        let gp = GRAMMAR.add(g as usize);
        println!(
            "grammar for: {}",
            if (*gp).module_name.is_null() {
                "<builtin>".to_string()
            } else {
                cstr_to_string((*gp).module_name)
            }
        );
        let mut s = 0;
        while s < (*gp).nspecs {
            let sp = (*gp).specs.add(s as usize);
            let arg = if (*sp).arg_type_idx != -1 {
                cstr_to_string((*TYPES.add((*sp).arg_type_idx as usize)).name)
            } else {
                "null".to_string()
            };
            println!("  {} ({})", cstr_to_string((*sp).keyword), arg);
            let mut r = 0;
            while r < (*sp).nrules {
                let rp = (*sp).rules.add(r as usize);
                println!(
                    "    {} {} {} ({:p})",
                    if (*rp).min_yang_version == YANG_VERSION_1_1 as c_char {
                        "1.1"
                    } else {
                        "   "
                    },
                    cstr_to_string((*rp).keyword),
                    (*rp).occurance as u8 as char,
                    (*rp).spec
                );
                r += 1;
            }
            s += 1;
        }
        g += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn yang_set_type_bits() {
    let atom_types = [
        "identifier",
        "uri",
        "boolean",
        "ordered-by-arg",
        "enum-arg",
        "deviate-arg",
        "=not-supported",
        "=delete",
        "=replace",
        "status-arg",
    ];
    let int_types = ["integer"];
    let uint_types = ["non-negative-integer", "fraction-digits-arg"];
    let atom_or_uint_types = ["max-value"];

    for name in atom_types {
        let c_name = match CString::new(name) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let idx = yang_get_arg_type_idx(yang_make_atom(c_name.as_ptr()));
        if idx >= 0 {
            let t = yang_get_arg_type(idx);
            if !t.is_null() {
                (*t).flags |= F_ERL_ATOM;
            }
        }
    }
    for name in int_types {
        let c_name = match CString::new(name) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let idx = yang_get_arg_type_idx(yang_make_atom(c_name.as_ptr()));
        if idx >= 0 {
            let t = yang_get_arg_type(idx);
            if !t.is_null() {
                (*t).flags |= F_ERL_INT;
            }
        }
    }
    for name in uint_types {
        let c_name = match CString::new(name) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let idx = yang_get_arg_type_idx(yang_make_atom(c_name.as_ptr()));
        if idx >= 0 {
            let t = yang_get_arg_type(idx);
            if !t.is_null() {
                (*t).flags |= F_ERL_UINT;
            }
        }
    }
    for name in atom_or_uint_types {
        let c_name = match CString::new(name) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let idx = yang_get_arg_type_idx(yang_make_atom(c_name.as_ptr()));
        if idx >= 0 {
            let t = yang_get_arg_type(idx);
            if !t.is_null() {
                (*t).flags |= F_ERL_ATOM_OR_UINT;
            }
        }
    }
    let idx = yang_get_arg_type_idx(yang_make_atom(c"identifier-ref".as_ptr()));
    if idx >= 0 {
        let t = yang_get_arg_type(idx);
        if !t.is_null() {
            (*t).flags |= F_ERL_IDENTIFIER_REF;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn yang_install_arg_types_str(stypes: *const *const c_char) -> bool {
    if stypes.is_null() {
        return false;
    }
    let mut n = 0usize;
    loop {
        let p = *stypes.add(n * 2);
        if p.is_null() {
            break;
        }
        n += 1;
    }
    if n == 0 {
        return true;
    }
    let mut tys: Vec<YangArgType> = Vec::with_capacity(n);
    for i in 0..n {
        let name = *stypes.add(i * 2);
        let re = *stypes.add(i * 2 + 1);
        tys.push(YangArgType {
            name: yang_make_atom(name),
            syntax: YangArgTypeSyntax {
                xsd_regexp: re as *mut c_char,
            },
            flags: F_ARG_TYPE_SYNTAX_REGEXP,
        });
    }
    yang_install_arg_types(tys.as_mut_ptr(), tys.len() as c_int)
}

#[no_mangle]
pub unsafe extern "C" fn yang_install_grammar_str(
    module_name: *const c_char,
    stmts: *const *const c_char,
) -> bool {
    if stmts.is_null() {
        return false;
    }

    let am_module_name = if module_name.is_null() {
        ptr::null_mut()
    } else {
        yang_make_atom(module_name)
    };

    let mut nspecs = 0usize;
    let mut nrules = 0usize;
    let mut i = 0usize;
    loop {
        let kw = *stmts.add(i);
        if kw.is_null() {
            break;
        }
        nspecs += 1;
        i += 2;
        while !(*stmts.add(i)).is_null() {
            nrules += 1;
            i += 3;
        }
        i += 3;
    }

    let mut specs: Vec<YangStatementSpec> = vec![
        YangStatementSpec {
            keyword: ptr::null_mut(),
            arg_type_idx: -1,
            flags: 0,
            rules: ptr::null_mut(),
            nrules: 0,
        };
        nspecs
    ];
    let mut rules: Vec<YangStatementRule> = vec![
        YangStatementRule {
            module_name: ptr::null_mut(),
            keyword: ptr::null_mut(),
            min_yang_version: YANG_VERSION_1 as c_char,
            spec: ptr::null_mut(),
            occurance: b'?' as c_char,
        };
        nrules
    ];

    i = 0;
    let mut s = 0usize;
    let mut r = 0usize;
    while !(*stmts.add(i)).is_null() {
        specs[s].keyword = yang_make_atom(*stmts.add(i));
        let arg_name = *stmts.add(i + 1);
        if !arg_name.is_null() {
            specs[s].arg_type_idx = yang_get_arg_type_idx(yang_make_atom(arg_name));
            if specs[s].arg_type_idx == -1 {
                return false;
            }
            if *arg_name == b'=' as c_char {
                specs[s].flags |= F_STMT_ARG_MATCH;
            }
        } else {
            specs[s].arg_type_idx = -1;
        }
        i += 2;
        specs[s].rules = rules.as_mut_ptr().add(r);
        let mut n = 0usize;
        while !(*stmts.add(i)).is_null() {
            rules[r].module_name = am_module_name;
            let vsn = *stmts.add(i);
            if CStr::from_ptr(vsn).to_bytes() == b"1.1" {
                rules[r].min_yang_version = YANG_VERSION_1_1 as c_char;
            } else {
                rules[r].min_yang_version = YANG_VERSION_1 as c_char;
            }
            rules[r].keyword = yang_make_atom(*stmts.add(i + 1));
            let occ = *stmts.add(i + 2);
            rules[r].occurance = if occ.is_null() { b'?' as c_char } else { *occ };
            i += 3;
            r += 1;
            n += 1;
        }
        specs[s].nrules = n as c_int;
        i += 3;
        s += 1;
    }

    yang_install_grammar(am_module_name, specs.as_mut_ptr(), specs.len() as c_int)
}
