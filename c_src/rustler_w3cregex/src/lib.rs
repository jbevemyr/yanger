use rustler::types::binary::Binary;
use rustler::{Atom, Encoder, Env, Error, NifResult, ResourceArc, Term};
use std::ffi::CString;
use std::os::raw::c_int;

#[repr(C)]
struct XmlRegexp {
    _private: [u8; 0],
}

extern "C" {
    fn xmlRegexpCompile(regexp: *const u8) -> *mut XmlRegexp;
    fn xmlRegexpExec(comp: *mut XmlRegexp, content: *const u8) -> c_int;
    fn xmlRegFreeRegexp(regexp: *mut XmlRegexp);
}

rustler::atoms! {
    ok,
    error,
    true_atom = "true",
    false_atom = "false"
}

struct RegexpResource {
    xreg_addr: usize,
    pattern: CString,
}

impl rustler::Resource for RegexpResource {}

impl Drop for RegexpResource {
    fn drop(&mut self) {
        if self.xreg_addr != 0 {
            // SAFETY: xreg_addr is created by xmlRegexpCompile and owned by this resource.
            unsafe {
                xmlRegFreeRegexp(self.xreg_addr as *mut XmlRegexp);
            }
            self.xreg_addr = 0;
        }
    }
}

impl RegexpResource {
    fn compile(pattern: &[u8]) -> Option<Self> {
        let pattern_cs = CString::new(pattern).ok()?;
        // SAFETY: xmlRegexpCompile expects a NUL-terminated string pointer.
        let xreg = unsafe { xmlRegexpCompile(pattern_cs.as_ptr() as *const u8) };
        if xreg.is_null() {
            return None;
        }
        Some(Self {
            xreg_addr: xreg as usize,
            pattern: pattern_cs,
        })
    }

    fn run_match(&self, input: &[u8]) -> c_int {
        let mut input_nt = Vec::with_capacity(input.len() + 1);
        input_nt.extend_from_slice(input);
        input_nt.push(0);
        // SAFETY: xreg_addr points to a valid compiled regex and input_nt has NUL terminator.
        unsafe { xmlRegexpExec(self.xreg_addr as *mut XmlRegexp, input_nt.as_ptr()) }
    }
}

unsafe impl Send for RegexpResource {}
unsafe impl Sync for RegexpResource {}

#[rustler::nif]
fn compile<'a>(env: Env<'a>, pattern: Binary<'a>) -> Term<'a> {
    match RegexpResource::compile(pattern.as_slice()) {
        Some(res) => (ok(), ResourceArc::new(res)).encode(env),
        None => (error(), "Bad Pattern").encode(env),
    }
}

fn match_result(code: c_int) -> Result<Atom, (Atom, &'static str)> {
    match code {
        1 => Ok(true_atom()),
        0 => Ok(false_atom()),
        _ => Err((error(), "?")),
    }
}

#[rustler::nif]
fn run_match<'a>(env: Env<'a>, r: ResourceArc<RegexpResource>, input: Binary<'a>) -> Term<'a> {
    match match_result(r.run_match(input.as_slice())) {
        Ok(atom) => atom.encode(env),
        Err(err) => err.encode(env),
    }
}

#[rustler::nif]
fn run_match_null<'a>(env: Env<'a>, r: ResourceArc<RegexpResource>, input: Binary<'a>) -> Term<'a> {
    match match_result(r.run_match(input.as_slice())) {
        Ok(atom) => atom.encode(env),
        Err(err) => err.encode(env),
    }
}

#[rustler::nif]
fn string(r: ResourceArc<RegexpResource>) -> NifResult<String> {
    let bytes = r.pattern.as_bytes().to_vec();
    String::from_utf8(bytes).map_err(|_| Error::BadArg)
}

#[rustler::nif]
fn is_xreg(_r: ResourceArc<RegexpResource>) -> Atom {
    true_atom()
}

fn load(env: Env, _term: Term) -> bool {
    env.register::<RegexpResource>().is_ok()
}

rustler::init!("w3cregex", load = load);
