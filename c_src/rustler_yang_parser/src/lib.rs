use rustler::types::binary::{Binary, OwnedBinary};
use rustler::{types::atom::Atom, types::list::ListIterator, Encoder, Env, Error, NifResult, Term};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

type YangAtom = *mut c_char;

#[repr(C)]
#[derive(Clone, Copy)]
struct YangArgTypeCb {
    validate: Option<unsafe extern "C" fn(*mut c_char, *mut c_void, c_char, *mut c_char, c_int) -> bool>,
    opaque: *mut c_void,
}

#[repr(C)]
union YangArgTypeSyntax {
    xsd_regexp: *mut c_char,
    cb: YangArgTypeCb,
}

#[repr(C)]
struct YangArgType {
    name: YangAtom,
    syntax: YangArgTypeSyntax,
    flags: u32,
}

#[repr(C)]
struct YangStatementRule {
    module_name: YangAtom,
    keyword: YangAtom,
    min_yang_version: c_char,
    spec: *mut YangStatementSpec,
    occurance: c_char,
}

#[repr(C)]
struct YangStatementSpec {
    keyword: YangAtom,
    arg_type_idx: c_int,
    flags: u32,
    rules: *mut YangStatementRule,
    nrules: c_int,
}

const F_ARG_TYPE_SYNTAX_REGEXP: u32 = 1 << 0;
const F_ERL_ATOM: u32 = 1 << 16;
const F_ERL_INT: u32 = 1 << 17;
const F_ERL_UINT: u32 = 1 << 18;
const F_ERL_IDENTIFIER_REF: u32 = 1 << 19;
const F_ERL_ATOM_OR_INT: u32 = 1 << 20;
const F_ERL_ATOM_OR_UINT: u32 = 1 << 21;
const YANG_VERSION_1: c_char = 1;
const YANG_FIRST_WARNING: c_int = 1000;

#[repr(C)]
struct YangStatement {
    prefix: YangAtom,
    module_name: YangAtom,
    keyword: YangAtom,
    arg: *mut c_char,
    arg_type: *mut c_void,
    yang_version: c_char,
    filename: *mut c_char,
    line: c_int,
    next: *mut YangStatement,
    substmt: *mut YangStatement,
}

#[repr(C)]
struct YangError {
    code: c_int,
    filename: *const c_char,
    line: c_int,
    col: c_int,
    msg: [c_char; libc::BUFSIZ as usize],
    next: *mut YangError,
}

#[repr(C)]
struct YangErrorCtx {
    err: *mut YangError,
}

extern "C" {
    fn yang_init_grammar() -> bool;
    fn yang_set_type_bits();
    fn yang_get_grammar_module_names(n: c_int, module_names: *mut *mut c_char) -> c_int;
    fn yang_make_atom(string: *const c_char) -> YangAtom;
    fn yang_get_statement_spec(module_name: YangAtom, keyword: YangAtom) -> *mut YangStatementSpec;
    fn yang_get_arg_type(arg_type_idx: c_int) -> *mut YangArgType;
    fn yang_get_arg_type_idx(name: YangAtom) -> c_int;
    fn yang_install_arg_types(types: *mut YangArgType, len: c_int) -> bool;
    fn yang_install_grammar(module_name: YangAtom, spec: *mut YangStatementSpec, len: c_int) -> bool;
    fn yang_add_rule_to_spec(rule: *mut YangStatementRule, module_name: YangAtom, keyword: YangAtom) -> bool;
    fn yang_alloc_err_ctx() -> *mut YangErrorCtx;
    fn yang_free_err_ctx(ectx: *mut YangErrorCtx);
    fn yang_parse(filename: *mut c_char, stmt: *mut *mut YangStatement, ectx: *mut YangErrorCtx) -> bool;
    fn yang_grammar_check_module(
        stmt: *mut YangStatement,
        canonical: bool,
        ectx: *mut YangErrorCtx,
    ) -> bool;
    fn yang_free_tree(stmt: *mut YangStatement);
}

rustler::atoms! {
    ok,
    error,
    value,
    undefined,
    not_found
}

#[rustler::nif]
fn get_grammar_module_names(env: Env) -> NifResult<Vec<Atom>> {
    // SAFETY: ffi call with null out-pointer to query required length.
    let n = unsafe { yang_get_grammar_module_names(0, ptr::null_mut()) };
    if n < 0 {
        return Err(Error::BadArg);
    }
    if n == 0 {
        return Ok(Vec::new());
    }

    let mut names: Vec<*mut c_char> = vec![ptr::null_mut(); n as usize];
    // SAFETY: names points to writable memory for n entries.
    let written = unsafe { yang_get_grammar_module_names(n, names.as_mut_ptr()) };
    if written < 0 {
        return Err(Error::BadArg);
    }

    let mut atoms = Vec::with_capacity(written as usize);
    for name_ptr in names.into_iter().take(written as usize) {
        if name_ptr.is_null() {
            continue;
        }
        // SAFETY: pointers are provided by grammar table and are NUL-terminated atom names.
        let name = unsafe { CStr::from_ptr(name_ptr) }
            .to_str()
            .map_err(|_| Error::BadArg)?;
        atoms.push(Atom::from_str(env, name)?);
    }
    Ok(atoms)
}

fn atom_to_yang_atom(env: Env, atom: Atom) -> NifResult<YangAtom> {
    // SAFETY: atom is a valid atom term in this environment.
    let name = unsafe { rustler::wrapper::atom::get_atom(env.as_c_arg(), atom.as_c_arg()) }?;
    let c_name = CString::new(name).map_err(|_| Error::BadArg)?;
    // SAFETY: yang_make_atom takes a valid NUL-terminated string and interns it.
    let a = unsafe { yang_make_atom(c_name.as_ptr()) };
    if a.is_null() {
        return Err(Error::BadArg);
    }
    Ok(a)
}

fn atom_text(env: Env, atom: Atom) -> NifResult<String> {
    // SAFETY: atom is a valid atom term in this environment.
    unsafe { rustler::wrapper::atom::get_atom(env.as_c_arg(), atom.as_c_arg()) }
}

fn parse_occurance(env: Env, occ_atom: Atom) -> NifResult<c_char> {
    let s = atom_text(env, occ_atom)?;
    if s.len() != 1 {
        return Err(Error::BadArg);
    }
    let b = s.as_bytes()[0];
    if matches!(b, b'?' | b'1' | b'*' | b'+') {
        Ok(b as c_char)
    } else {
        Err(Error::BadArg)
    }
}

fn parse_keyword_term(env: Env, kw_term: Term) -> NifResult<(YangAtom, YangAtom)> {
    if kw_term.is_atom() {
        let kw: Atom = kw_term.decode()?;
        Ok((ptr::null_mut(), atom_to_yang_atom(env, kw)?))
    } else {
        let (module, kw): (Atom, Atom) = kw_term.decode()?;
        Ok((atom_to_yang_atom(env, module)?, atom_to_yang_atom(env, kw)?))
    }
}

fn term_to_bytes_latin1(term: Term) -> NifResult<Vec<u8>> {
    if let Ok(bin) = term.decode::<Binary>() {
        return Ok(bin.as_slice().to_vec());
    }
    let list_iter: ListIterator = term.decode()?;
    let mut bytes = Vec::new();
    for elem in list_iter {
        let codepoint: i64 = elem.decode()?;
        if !(0..=255).contains(&codepoint) {
            return Err(Error::BadArg);
        }
        bytes.push(codepoint as u8);
    }
    Ok(bytes)
}

fn bytes_to_charlist_term<'a>(env: Env<'a>, bytes: &[u8]) -> Term<'a> {
    bytes.to_vec().encode(env)
}

fn cstr_to_charlist_term<'a>(env: Env<'a>, p: *const c_char) -> Term<'a> {
    if p.is_null() {
        return Vec::<u8>::new().encode(env);
    }
    // SAFETY: p points to a NUL-terminated C string.
    let bytes = unsafe { CStr::from_ptr(p) }.to_bytes();
    bytes_to_charlist_term(env, bytes)
}

fn binary_from_bytes_term<'a>(env: Env<'a>, bytes: &[u8]) -> NifResult<Term<'a>> {
    let mut owned = OwnedBinary::new(bytes.len()).ok_or(Error::BadArg)?;
    owned.as_mut_slice().copy_from_slice(bytes);
    Ok(owned.release(env).to_term(env))
}

fn arg_to_term<'a>(env: Env<'a>, stmt: &YangStatement) -> NifResult<Term<'a>> {
    if stmt.arg.is_null() {
        return Ok(Vec::<Term<'a>>::new().encode(env));
    }
    if stmt.arg_type.is_null() {
        // SAFETY: arg is a valid NUL-terminated string from parser.
        return binary_from_bytes_term(env, unsafe { CStr::from_ptr(stmt.arg) }.to_bytes());
    }
    let arg_type = stmt.arg_type as *const YangArgType;
    // SAFETY: parser sets arg_type to a valid pointer or null.
    let flags = unsafe { (*arg_type).flags };
    // SAFETY: arg is a valid NUL-terminated string from parser.
    let arg_bytes = unsafe { CStr::from_ptr(stmt.arg) }.to_bytes();

    if (flags & F_ERL_ATOM) != 0 {
        let s = std::str::from_utf8(arg_bytes).map_err(|_| Error::BadArg)?;
        return Ok(Atom::from_str(env, s)?.to_term(env));
    }
    if (flags & F_ERL_INT) != 0 {
        let s = std::str::from_utf8(arg_bytes).map_err(|_| Error::BadArg)?;
        let v: i64 = s.parse().map_err(|_| Error::BadArg)?;
        return Ok(v.encode(env));
    }
    if (flags & F_ERL_UINT) != 0 {
        let s = std::str::from_utf8(arg_bytes).map_err(|_| Error::BadArg)?;
        let v: u64 = s.parse().map_err(|_| Error::BadArg)?;
        return Ok(v.encode(env));
    }
    if (flags & F_ERL_ATOM_OR_INT) != 0 {
        let s = std::str::from_utf8(arg_bytes).map_err(|_| Error::BadArg)?;
        if let Ok(v) = s.parse::<i64>() {
            return Ok(v.encode(env));
        }
        return Ok(Atom::from_str(env, s)?.to_term(env));
    }
    if (flags & F_ERL_ATOM_OR_UINT) != 0 {
        let s = std::str::from_utf8(arg_bytes).map_err(|_| Error::BadArg)?;
        if let Ok(v) = s.parse::<u64>() {
            return Ok(v.encode(env));
        }
        return Ok(Atom::from_str(env, s)?.to_term(env));
    }
    if (flags & F_ERL_IDENTIFIER_REF) != 0 {
        let s = std::str::from_utf8(arg_bytes).map_err(|_| Error::BadArg)?;
        if let Some((prefix, name)) = s.split_once(':') {
            return Ok((Atom::from_str(env, prefix)?, Atom::from_str(env, name)?).encode(env));
        }
        return Ok(Atom::from_str(env, s)?.to_term(env));
    }

    binary_from_bytes_term(env, arg_bytes)
}

fn mk_tree<'a>(env: Env<'a>, stmt: *mut YangStatement, fname_term: Term<'a>) -> NifResult<Term<'a>> {
    let mut out = Vec::new();
    let mut cur = stmt;
    while !cur.is_null() {
        // SAFETY: cur traverses parser-owned linked list.
        let s = unsafe { &*cur };
        let keyword_term = if s.prefix.is_null() {
            // SAFETY: keyword is valid atom string.
            let kw = unsafe { CStr::from_ptr(s.keyword) }
                .to_str()
                .map_err(|_| Error::BadArg)?;
            Atom::from_str(env, kw)?.to_term(env)
        } else {
            // SAFETY: module_name/keyword are valid atom strings.
            let m = unsafe { CStr::from_ptr(s.module_name) }
                .to_str()
                .map_err(|_| Error::BadArg)?;
            let k = unsafe { CStr::from_ptr(s.keyword) }
                .to_str()
                .map_err(|_| Error::BadArg)?;
            (Atom::from_str(env, m)?, Atom::from_str(env, k)?).encode(env)
        };
        let arg_term = arg_to_term(env, s)?;
        let line_term = (fname_term, s.line).encode(env);
        let sub_term = mk_tree(env, s.substmt, fname_term)?;
        out.push((keyword_term, arg_term, line_term, sub_term).encode(env));
        cur = s.next;
    }
    Ok(out.encode(env))
}

fn mk_error_list<'a>(env: Env<'a>, err: *mut YangError) -> NifResult<Term<'a>> {
    let mut terms = Vec::new();
    let mut cur = err;
    while !cur.is_null() {
        // SAFETY: cur traverses error linked list owned by context.
        let e = unsafe { &*cur };
        let fname_term = cstr_to_charlist_term(env, e.filename);
        let msg_end = e
            .msg
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(e.msg.len());
        let msg_bytes: Vec<u8> = e.msg[..msg_end].iter().map(|c| *c as u8).collect();
        let msg_term = bytes_to_charlist_term(env, &msg_bytes);
        terms.push((e.code, fname_term, e.line, e.col, msg_term).encode(env));
        cur = e.next;
    }
    terms.reverse();
    Ok(terms.encode(env))
}

fn rule_keyword_term<'a>(env: Env<'a>, rule: &YangStatementRule) -> NifResult<Term<'a>> {
    // SAFETY: keyword pointers in grammar are valid atom strings.
    let kw = unsafe { CStr::from_ptr(rule.keyword) }
        .to_str()
        .map_err(|_| Error::BadArg)?;
    let kw_atom = Atom::from_str(env, kw)?;
    if rule.module_name.is_null() {
        Ok(kw_atom.to_term(env))
    } else {
        // SAFETY: module_name pointers in grammar are valid atom strings.
        let module = unsafe { CStr::from_ptr(rule.module_name) }
            .to_str()
            .map_err(|_| Error::BadArg)?;
        let module_atom = Atom::from_str(env, module)?;
        Ok((module_atom, kw_atom).encode(env))
    }
}

#[rustler::nif]
fn get_statement_spec<'a>(env: Env<'a>, keyword_term: Term<'a>) -> NifResult<Term<'a>> {
    let (module_name, keyword) = if keyword_term.is_atom() {
        let kw_atom: Atom = keyword_term.decode()?;
        (ptr::null_mut(), atom_to_yang_atom(env, kw_atom)?)
    } else {
        let (mod_atom, kw_atom): (Atom, Atom) = keyword_term.decode()?;
        (atom_to_yang_atom(env, mod_atom)?, atom_to_yang_atom(env, kw_atom)?)
    };

    // SAFETY: module_name/keyword are interned atoms used by grammar lookup.
    let spec = unsafe { yang_get_statement_spec(module_name, keyword) };
    if spec.is_null() {
        return Ok(not_found().to_term(env));
    }

    // SAFETY: spec pointer returned by grammar lookup is valid while process is alive.
    let spec_ref = unsafe { &*spec };

    // SAFETY: keyword pointer is a valid NUL-terminated atom name.
    let kw_name = unsafe { CStr::from_ptr(spec_ref.keyword) }
        .to_str()
        .map_err(|_| Error::BadArg)?;
    let kw_atom = Atom::from_str(env, kw_name)?;

    let arg_term = if spec_ref.arg_type_idx < 0 {
        Vec::<Term<'a>>::new().encode(env)
    } else {
        // SAFETY: arg_type_idx is provided by installed grammar and checked by callee.
        let arg_type = unsafe { yang_get_arg_type(spec_ref.arg_type_idx) };
        if arg_type.is_null() {
            Vec::<Term<'a>>::new().encode(env)
        } else {
            // SAFETY: name is an interned atom string.
            let arg_name = unsafe { CStr::from_ptr((*arg_type).name) }
                .to_str()
                .map_err(|_| Error::BadArg)?;
            Atom::from_str(env, arg_name)?.to_term(env)
        }
    };

    let mut rules = Vec::with_capacity(spec_ref.nrules.max(0) as usize);
    for i in 0..spec_ref.nrules {
        // SAFETY: rules points to an array with nrules entries.
        let rule_ref = unsafe { &*spec_ref.rules.add(i as usize) };
        let kw_term = rule_keyword_term(env, rule_ref)?;
        let occ = (rule_ref.occurance as u8 as char).to_string();
        let occ_atom = Atom::from_str(env, &occ)?;
        rules.push((kw_term, occ_atom).encode(env));
    }

    Ok((value(), (kw_atom, arg_term, rules, undefined())).encode(env))
}

#[rustler::nif]
fn parse<'a>(env: Env<'a>, filename_term: Term<'a>, canonical_term: Atom) -> NifResult<Term<'a>> {
    let filename_bytes = term_to_bytes_latin1(filename_term)?;
    let filename_c = CString::new(filename_bytes.clone()).map_err(|_| Error::BadArg)?;
    let canonical = if canonical_term == Atom::from_str(env, "true")? {
        true
    } else if canonical_term == Atom::from_str(env, "false")? {
        false
    } else {
        return Err(Error::BadArg);
    };

    // SAFETY: allocates context owned by this call and later freed.
    let ectx = unsafe { yang_alloc_err_ctx() };
    if ectx.is_null() {
        return Err(Error::BadArg);
    }
    let mut stmt: *mut YangStatement = ptr::null_mut();
    // SAFETY: pointers are valid for the call duration.
    let parsed = unsafe { yang_parse(filename_c.as_ptr() as *mut c_char, &mut stmt, ectx) };
    if parsed {
        // SAFETY: stmt and ectx were produced by parser.
        unsafe {
            yang_grammar_check_module(stmt, canonical, ectx);
        }
    }

    let mut has_error = false;
    // SAFETY: ectx points to a valid error context until freed below.
    let mut err = unsafe { (*ectx).err };
    while !err.is_null() && !has_error {
        // SAFETY: err traverses valid linked list.
        let e = unsafe { &*err };
        if e.code < YANG_FIRST_WARNING {
            has_error = true;
        }
        err = e.next;
    }

    // SAFETY: ectx is valid and still alive here.
    let errors_term = mk_error_list(env, unsafe { (*ectx).err })?;
    let result = if !parsed || has_error {
        (Atom::from_str(env, "error")?, errors_term).encode(env)
    } else {
        let fname_term = bytes_to_charlist_term(env, &filename_bytes);
        let tree_term = mk_tree(env, stmt, fname_term)?;
        (ok(), tree_term, errors_term).encode(env)
    };

    // SAFETY: free parser tree/context exactly once.
    unsafe {
        if !stmt.is_null() {
            yang_free_tree(stmt);
        }
        yang_free_err_ctx(ectx);
    }
    Ok(result)
}

#[rustler::nif]
fn install_arg_types<'a>(env: Env<'a>, types: Vec<(Atom, Term<'a>, Atom)>) -> NifResult<Atom> {
    let mut c_types = Vec::with_capacity(types.len());
    let mut regex_storage: Vec<CString> = Vec::new();

    for (name_atom, regexp_term, return_type_atom) in types {
        let name = atom_to_yang_atom(env, name_atom)?;
        let mut flags: u32 = 0;
        let syntax: YangArgTypeSyntax;

        if let Ok(a) = regexp_term.decode::<Atom>() {
            if a != undefined() {
                return Err(Error::BadArg);
            }
            syntax = YangArgTypeSyntax {
                xsd_regexp: ptr::null_mut(),
            };
        } else {
            let regex_bytes = if let Ok(re) = regexp_term.decode::<String>() {
                re.into_bytes()
            } else if let Ok(list_iter) = regexp_term.decode::<ListIterator>() {
                let mut bytes = Vec::new();
                for elem in list_iter {
                    let codepoint: i64 = elem.decode()?;
                    if !(0..=255).contains(&codepoint) {
                        return Err(Error::BadArg);
                    }
                    bytes.push(codepoint as u8);
                }
                bytes
            } else {
                return Err(Error::BadArg);
            };

            let c_re = CString::new(regex_bytes).map_err(|_| Error::BadArg)?;
            let re_ptr = c_re.as_ptr() as *mut c_char;
            regex_storage.push(c_re);
            flags |= F_ARG_TYPE_SYNTAX_REGEXP;
            syntax = YangArgTypeSyntax { xsd_regexp: re_ptr };
        }

        // SAFETY: return_type_atom is a valid atom term in this environment.
        let return_type =
            unsafe { rustler::wrapper::atom::get_atom(env.as_c_arg(), return_type_atom.as_c_arg()) }?;
        match return_type.as_str() {
            "string" => {}
            "atom" => flags |= F_ERL_ATOM,
            "int" => flags |= F_ERL_INT,
            "atom-or-int" => flags |= F_ERL_ATOM_OR_INT,
            "identifier-ref" => flags |= F_ERL_IDENTIFIER_REF,
            _ => return Err(Error::BadArg),
        }

        c_types.push(YangArgType { name, syntax, flags });
    }

    // SAFETY: c_types points to a contiguous array of C-compatible structs for this call.
    let installed = unsafe { yang_install_arg_types(c_types.as_mut_ptr(), c_types.len() as c_int) };
    if installed {
        Ok(ok())
    } else {
        Ok(error())
    }
}

#[derive(Default)]
struct UseInSpec {
    occ: c_char,
    keywords: Vec<(YangAtom, YangAtom)>,
}

#[rustler::nif]
fn install_grammar<'a>(env: Env<'a>, module_name: Atom, specs_term: Term<'a>) -> NifResult<Atom> {
    let module_name = atom_to_yang_atom(env, module_name)?;
    let specs_iter: ListIterator = specs_term.decode()?;

    let mut specs: Vec<YangStatementSpec> = Vec::new();
    let mut rules: Vec<YangStatementRule> = Vec::new();
    let mut usein_specs: Vec<Option<UseInSpec>> = Vec::new();

    for spec_term in specs_iter {
        let (keyword_term, arg_term, rules_term, usein_term): (Term, Term, Term, Term) =
            spec_term.decode()?;
        let keyword_atom: Atom = keyword_term.decode()?;
        let keyword = atom_to_yang_atom(env, keyword_atom)?;

        let arg_type_idx = if arg_term.is_empty_list() {
            -1
        } else {
            let arg_atom: Atom = arg_term.decode()?;
            let arg_name = atom_to_yang_atom(env, arg_atom)?;
            // SAFETY: arg_name is an interned atom pointer from the shared atom table.
            let idx = unsafe { yang_get_arg_type_idx(arg_name) };
            if idx < 0 {
                return Err(Error::BadArg);
            }
            idx
        };

        let rule_start = rules.len();
        let rules_iter: ListIterator = rules_term.decode()?;
        for rule_term in rules_iter {
            let (kw_term, occ_term): (Term, Atom) = rule_term.decode()?;
            let (sub_module, sub_keyword) = parse_keyword_term(env, kw_term)?;
            let occ = parse_occurance(env, occ_term)?;
            rules.push(YangStatementRule {
                module_name: sub_module,
                keyword: sub_keyword,
                min_yang_version: 0,
                spec: ptr::null_mut(),
                occurance: occ,
            });
        }
        let nrules = (rules.len() - rule_start) as c_int;
        let rules_ptr = if nrules == 0 {
            ptr::null_mut()
        } else {
            // SAFETY: rule_start is within bounds and vector won't reallocate before we finish.
            unsafe { rules.as_mut_ptr().add(rule_start) }
        };

        let usein = if usein_term.is_atom() {
            let atom: Atom = usein_term.decode()?;
            if atom != undefined() {
                return Err(Error::BadArg);
            }
            None
        } else {
            let (occ_atom, kws_term): (Atom, Term) = usein_term.decode()?;
            let occ = parse_occurance(env, occ_atom)?;
            let mut kws = Vec::new();
            let kws_iter: ListIterator = kws_term.decode()?;
            for kw_term in kws_iter {
                kws.push(parse_keyword_term(env, kw_term)?);
            }
            Some(UseInSpec { occ, keywords: kws })
        };

        specs.push(YangStatementSpec {
            keyword,
            arg_type_idx,
            flags: 0,
            rules: rules_ptr,
            nrules,
        });
        usein_specs.push(usein);
    }

    // Fix rules pointers in case vector growth changed base address while decoding.
    let mut base = 0usize;
    for spec in &mut specs {
        if spec.nrules == 0 {
            spec.rules = ptr::null_mut();
        } else {
            // SAFETY: base is advanced by each spec.nrules and stays within rules length.
            spec.rules = unsafe { rules.as_mut_ptr().add(base) };
        }
        base += spec.nrules.max(0) as usize;
    }

    // SAFETY: specs/rules are C-compatible buffers valid for duration of the call.
    let ok_install =
        unsafe { yang_install_grammar(module_name, specs.as_mut_ptr(), specs.len() as c_int) };
    if !ok_install {
        return Ok(error());
    }

    for (idx, usein_opt) in usein_specs.iter().enumerate() {
        let Some(usein) = usein_opt else { continue };
        let mut rule = YangStatementRule {
            module_name,
            keyword: specs[idx].keyword,
            min_yang_version: YANG_VERSION_1,
            spec: ptr::null_mut(),
            occurance: usein.occ,
        };
        // SAFETY: we just installed this grammar spec for module_name + keyword.
        rule.spec = unsafe { yang_get_statement_spec(module_name, rule.keyword) };
        if rule.spec.is_null() {
            return Ok(error());
        }
        for (usein_module, usein_keyword) in &usein.keywords {
            // SAFETY: rule points to initialized local rule; keywords are interned atoms.
            let ok_add =
                unsafe { yang_add_rule_to_spec(&mut rule, *usein_module, *usein_keyword) };
            if !ok_add {
                return Ok(error());
            }
        }
    }

    Ok(ok())
}

fn load(_env: Env, _term: rustler::Term) -> bool {
    // SAFETY: these initialize global grammar/type data exactly like current C NIF load callback.
    unsafe {
        if !yang_init_grammar() {
            return false;
        }
        yang_set_type_bits();
    }
    true
}

rustler::init!("yang_parser_rustler", load = load);
