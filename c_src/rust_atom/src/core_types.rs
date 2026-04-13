use crate::core_validators::{
    chk_date_rs, chk_enum_arg_rs, chk_fraction_digits_arg_rs, chk_identifier_rs,
    chk_if_feature_expr_rs, chk_integer_rs, chk_max_value_rs, chk_non_negative_integer_rs,
};
use crate::grammar::{yang_install_arg_types, YangArgType};
use crate::yang_make_atom;
use std::ffi::CString;
use std::os::raw::{c_int, c_void};
use std::sync::OnceLock;

const F_ARG_TYPE_SYNTAX_REGEXP: u32 = 1 << 0;
const F_ARG_TYPE_SYNTAX_CB: u32 = 1 << 1;
const IDENTIFIER_RE: &[u8] = b"[_A-Za-z][._\\-A-Za-z0-9]*\0";
const DATE_RE: &[u8] = b"[1-2][0-9]{3}-(0[1-9]|1[012])-(0[1-9]|[12][0-9]|3[01])\0";
const IDENTIFIER_REF_RE: &[u8] = b"(([_A-Za-z][._\\-A-Za-z0-9]*):)?([_A-Za-z][._\\-A-Za-z0-9]*)\0";
const LENGTH_ARG_RE: &[u8] = b"((min|max|[0-9]+)\\s*(\\.\\.\\s*(min|max|[0-9]+)\\s*)?)(\\|\\s*((min|max|[0-9]+)\\s*(\\.\\.\\s*(min|max|[0-9]+)\\s*)?))*\0";
const RANGE_ARG_RE: &[u8] = b"((\\-INF|min|max|((\\+|\\-)?[0-9]+(\\.[0-9]+)?))\\s*(\\.\\.\\s*(INF|min|max|(\\+|\\-)?[0-9]+(\\.[0-9]+)?)\\s*)?)(\\|\\s*((\\-INF|min|max|((\\+|\\-)?[0-9]+(\\.[0-9]+)?))\\s*(\\.\\.\\s*(INF|min|max|(\\+|\\-)?[0-9]+(\\.[0-9]+)?)\\s*)?))*\0";
const KEY_ARG_RE: &[u8] = b"(([_A-Za-z][._\\-A-Za-z0-9]*):)?([_A-Za-z][._\\-A-Za-z0-9]*)(\\s+(([_A-Za-z][._\\-A-Za-z0-9]*):)?([_A-Za-z][._\\-A-Za-z0-9]*))*\0";

const IDX_STRING: usize = 0;
const IDX_IDENTIFIER: usize = 1;
const IDX_VERSION: usize = 2;
const IDX_DATE: usize = 3;
const IDX_ORDERED_BY_ARG: usize = 4;
const IDX_BOOLEAN: usize = 5;
const IDX_MAX_VALUE: usize = 6;
const IDX_NON_NEGATIVE_INTEGER: usize = 7;
const IDX_DEVIATE_ARG: usize = 8;
const IDX_NOT_SUPPORTED: usize = 9;
const IDX_DELETE: usize = 10;
const IDX_REPLACE: usize = 11;
const IDX_INTEGER: usize = 12;
const IDX_STATUS_ARG: usize = 13;
const IDX_MODIFIER_ARG: usize = 14;
const IDX_URI: usize = 15;
const IDX_LENGTH_ARG: usize = 16;
const IDX_KEY_ARG: usize = 17;
const IDX_DESCENDANT_SCHEMA_NODEID: usize = 18;
const IDX_ABSOLUTE_SCHEMA_NODEID: usize = 19;
const IDX_ENUM_ARG: usize = 20;
const IDX_RANGE_ARG: usize = 21;
const IDX_IDENTIFIER_REF: usize = 22;
const IDX_FRACTION_DIGITS_ARG: usize = 23;
const IDX_UNIQUE_ARG: usize = 24;
const IDX_PATH_ARG: usize = 25;
const IDX_STRICT_PATH_ARG: usize = 26;
const IDX_SCHEMA_NODEID: usize = 27;
const IDX_IF_FEATURE_EXPR: usize = 28;
const CORE_TYPES_COUNT: usize = 29;

struct CoreRegexPtrs {
    uri: CString,
    descendant_schema_nodeid: CString,
    absolute_schema_nodeid: CString,
    unique_arg: CString,
    path_arg: CString,
    strict_path_arg: CString,
    schema_nodeid: CString,
}

#[repr(C)]
struct XmlRegexp {
    _private: [u8; 0],
}

extern "C" {
    fn xmlRegexpCompile(regexp: *const u8) -> *mut XmlRegexp;
}

fn core_regex_ptrs() -> &'static CoreRegexPtrs {
    static PTRS: OnceLock<CoreRegexPtrs> = OnceLock::new();
    PTRS.get_or_init(|| {
        let identifier = "[_A-Za-z][._\\-A-Za-z0-9]*";
        let node_id = format!("(({}):)?({})", identifier, identifier);
        let rel_path_keyexpr = format!("(\\.\\./)+({}/)*{}", node_id, node_id);
        let path_key_expr = format!("(current\\s*\\(\\s*\\)/{})", rel_path_keyexpr);
        let path_equality_expr = format!("{}\\s*=\\s*{}", node_id, path_key_expr);
        let path_predicate = format!("\\s*\\[\\s*{}\\s*\\]\\s*", path_equality_expr);
        let absolute_path_arg = format!("(/{0}({1})*)+", node_id, path_predicate);
        let descendant_path_arg =
            format!("{}({})*({})?", node_id, path_predicate, absolute_path_arg);
        let relative_path_arg = format!("(\\.\\./)*{}", descendant_path_arg);
        let deref_path_arg = format!("deref\\s*\\(\\s*({0})\\s*\\)/\\.\\./{0}", relative_path_arg);
        let path_arg = format!(
            "({}|{}|{})",
            absolute_path_arg, relative_path_arg, deref_path_arg
        );
        let strict_path_arg = format!("({}|{})", absolute_path_arg, relative_path_arg);
        // URI regex copied from C builder to preserve behavior.
        let scheme = "[A-Za-z][-+.A-Za-z0-9]*";
        let unreserved = "[-._~A-Za-z0-9]";
        let pct_encoded = "%[0-9A-F]{2}";
        let sub_delims = "[!$&'()*+,;=]";
        let dec_octet = "([0-9]|[1-9][0-9]|1[0-9]{2}|2[0-4][0-9]|25[0-5])";
        let h16 = "[0-9A-F]{1,4}";
        let port = "[0-9]*";
        let pchar = format!("({}|{}|{}|[:@])", unreserved, pct_encoded, sub_delims);
        let segment = format!("{}*", pchar);
        let segment_nz = format!("{}+", pchar);
        let userinfo = format!("({}|{}|{}|:)*", unreserved, pct_encoded, sub_delims);
        let ipv4address = format!("({0}.){{3}}{0}", dec_octet);
        let ls32 = format!("({}:{}|{})", h16, h16, ipv4address);
        let ipv6address = format!(
            "(({0}:){{6}}{1}|::({0}:){{5}}{1}|({0})?::({0}:){{4}}{1}|(({0}:)?{0})?::({0}:){{3}}{1}|(({0}:){{0,2}}{0})?::({0}:){{2}}{1}|(({0}:){{0,3}}{0})?::{0}:{1}|(({0}:){{0,4}}{0})?::{1}|(({0}:){{0,5}}{0})?::{1}|(({0}:){{0,6}}{0})?::)",
            h16, ls32
        );
        let ipvfuture = format!("v[0-9A-F]+\\.({}|{}|:)+", unreserved, sub_delims);
        let ipliteral = format!("\\[({}|{})\\]", ipv6address, ipvfuture);
        let reg_name = format!("({}|{}|{})*", unreserved, pct_encoded, sub_delims);
        let host = format!("({}|{}|{})", ipliteral, ipv4address, reg_name);
        let authority = format!("({}@)?{}(:{})?", userinfo, host, port);
        let path_abempty = format!("(/{} )*", segment).replace(" ", "");
        let path_absolute = format!("/({}(/{} )*)?", segment_nz, segment).replace(" ", "");
        let path_rootless = format!("{}(/{} )*", segment_nz, segment).replace(" ", "");
        let path_empty = format!("{}{{0}}", pchar);
        let hier_part = format!(
            "(//{}{}|{}|{}|{})",
            authority, path_abempty, path_absolute, path_rootless, path_empty
        );
        let query = format!("({}|[/?])*", pchar);
        let fragment = query.clone();
        let uri = format!("{}:{}(\\?{})?(#{})?", scheme, hier_part, query, fragment);
        let absolute_schema_nodeid = format!("(/{})+", node_id);
        let descendant_schema_nodeid = format!("{}({})?", node_id, absolute_schema_nodeid);
        let unique_arg = format!(
            "{}(\\s+{})*",
            descendant_schema_nodeid, descendant_schema_nodeid
        );
        let schema_nodeid = format!("({}|{})", absolute_schema_nodeid, descendant_schema_nodeid);
        CoreRegexPtrs {
            uri: CString::new(uri).expect("regex contains interior NUL"),
            descendant_schema_nodeid: CString::new(descendant_schema_nodeid)
                .expect("regex contains interior NUL"),
            absolute_schema_nodeid: CString::new(absolute_schema_nodeid)
                .expect("regex contains interior NUL"),
            unique_arg: CString::new(unique_arg).expect("regex contains interior NUL"),
            path_arg: CString::new(path_arg).expect("regex contains interior NUL"),
            strict_path_arg: CString::new(strict_path_arg).expect("regex contains interior NUL"),
            schema_nodeid: CString::new(schema_nodeid).expect("regex contains interior NUL"),
        }
    })
}

pub unsafe fn init_core_stmt_types() -> c_int {
    let mut types: Vec<YangArgType> = vec![std::mem::zeroed(); CORE_TYPES_COUNT];
    fill_simple_core_types(&mut types, CORE_TYPES_COUNT);
    if !yang_install_arg_types(types.as_mut_ptr(), CORE_TYPES_COUNT as c_int) {
        return 0;
    }
    1
}

unsafe fn fill_simple_core_types(types: &mut [YangArgType], ntypes: usize) {
    let regex_ptrs = core_regex_ptrs();

    if IDX_STRING < ntypes {
        types[IDX_STRING].name = yang_make_atom(c"string".as_ptr());
        types[IDX_STRING].flags = 0;
    }
    if IDX_IDENTIFIER < ntypes {
        types[IDX_IDENTIFIER].name = yang_make_atom(c"identifier".as_ptr());
        types[IDX_IDENTIFIER].syntax.cb.validate = Some(chk_identifier_rs);
        types[IDX_IDENTIFIER].syntax.cb.opaque =
            xmlRegexpCompile(IDENTIFIER_RE.as_ptr()) as *mut c_void;
        types[IDX_IDENTIFIER].flags = F_ARG_TYPE_SYNTAX_CB;
    }
    if IDX_VERSION < ntypes {
        types[IDX_VERSION].name = yang_make_atom(c"version".as_ptr());
        types[IDX_VERSION].syntax.xsd_regexp = c"1|1\\.1".as_ptr() as *mut _;
        types[IDX_VERSION].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_DATE < ntypes {
        types[IDX_DATE].name = yang_make_atom(c"date".as_ptr());
        types[IDX_DATE].syntax.cb.validate = Some(chk_date_rs);
        types[IDX_DATE].syntax.cb.opaque = xmlRegexpCompile(DATE_RE.as_ptr()) as *mut c_void;
        types[IDX_DATE].flags = F_ARG_TYPE_SYNTAX_CB;
    }
    if IDX_ORDERED_BY_ARG < ntypes {
        types[IDX_ORDERED_BY_ARG].name = yang_make_atom(c"ordered-by-arg".as_ptr());
        types[IDX_ORDERED_BY_ARG].syntax.xsd_regexp = c"user|system".as_ptr() as *mut _;
        types[IDX_ORDERED_BY_ARG].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_BOOLEAN < ntypes {
        types[IDX_BOOLEAN].name = yang_make_atom(c"boolean".as_ptr());
        types[IDX_BOOLEAN].syntax.xsd_regexp = c"true|false".as_ptr() as *mut _;
        types[IDX_BOOLEAN].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_MAX_VALUE < ntypes {
        types[IDX_MAX_VALUE].name = yang_make_atom(c"max-value".as_ptr());
        types[IDX_MAX_VALUE].syntax.cb.validate = Some(chk_max_value_rs);
        types[IDX_MAX_VALUE].flags = F_ARG_TYPE_SYNTAX_CB;
    }
    if IDX_NON_NEGATIVE_INTEGER < ntypes {
        types[IDX_NON_NEGATIVE_INTEGER].name = yang_make_atom(c"non-negative-integer".as_ptr());
        types[IDX_NON_NEGATIVE_INTEGER].syntax.cb.validate = Some(chk_non_negative_integer_rs);
        types[IDX_NON_NEGATIVE_INTEGER].flags = F_ARG_TYPE_SYNTAX_CB;
    }
    if IDX_DEVIATE_ARG < ntypes {
        types[IDX_DEVIATE_ARG].name = yang_make_atom(c"deviate-arg".as_ptr());
        types[IDX_DEVIATE_ARG].syntax.xsd_regexp =
            c"add|delete|replace|not-supported".as_ptr() as *mut _;
        types[IDX_DEVIATE_ARG].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_NOT_SUPPORTED < ntypes {
        types[IDX_NOT_SUPPORTED].name = yang_make_atom(c"=not-supported".as_ptr());
        types[IDX_NOT_SUPPORTED].syntax.xsd_regexp = c"not-supported".as_ptr() as *mut _;
        types[IDX_NOT_SUPPORTED].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_DELETE < ntypes {
        types[IDX_DELETE].name = yang_make_atom(c"=delete".as_ptr());
        types[IDX_DELETE].syntax.xsd_regexp = c"delete".as_ptr() as *mut _;
        types[IDX_DELETE].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_REPLACE < ntypes {
        types[IDX_REPLACE].name = yang_make_atom(c"=replace".as_ptr());
        types[IDX_REPLACE].syntax.xsd_regexp = c"replace".as_ptr() as *mut _;
        types[IDX_REPLACE].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_INTEGER < ntypes {
        types[IDX_INTEGER].name = yang_make_atom(c"integer".as_ptr());
        types[IDX_INTEGER].syntax.cb.validate = Some(chk_integer_rs);
        types[IDX_INTEGER].flags = F_ARG_TYPE_SYNTAX_CB;
    }
    if IDX_STATUS_ARG < ntypes {
        types[IDX_STATUS_ARG].name = yang_make_atom(c"status-arg".as_ptr());
        types[IDX_STATUS_ARG].syntax.xsd_regexp = c"current|obsolete|deprecated".as_ptr() as *mut _;
        types[IDX_STATUS_ARG].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_MODIFIER_ARG < ntypes {
        types[IDX_MODIFIER_ARG].name = yang_make_atom(c"modifier-arg".as_ptr());
        types[IDX_MODIFIER_ARG].syntax.xsd_regexp = c"invert-match".as_ptr() as *mut _;
        types[IDX_MODIFIER_ARG].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_URI < ntypes {
        types[IDX_URI].name = yang_make_atom(c"uri".as_ptr());
        types[IDX_URI].syntax.xsd_regexp = regex_ptrs.uri.as_ptr() as *mut _;
        types[IDX_URI].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_LENGTH_ARG < ntypes {
        types[IDX_LENGTH_ARG].name = yang_make_atom(c"length-arg".as_ptr());
        types[IDX_LENGTH_ARG].syntax.xsd_regexp = LENGTH_ARG_RE.as_ptr() as *mut _;
        types[IDX_LENGTH_ARG].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_KEY_ARG < ntypes {
        types[IDX_KEY_ARG].name = yang_make_atom(c"key-arg".as_ptr());
        types[IDX_KEY_ARG].syntax.xsd_regexp = KEY_ARG_RE.as_ptr() as *mut _;
        types[IDX_KEY_ARG].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_DESCENDANT_SCHEMA_NODEID < ntypes {
        types[IDX_DESCENDANT_SCHEMA_NODEID].name =
            yang_make_atom(c"descendant-schema-nodeid".as_ptr());
        types[IDX_DESCENDANT_SCHEMA_NODEID].syntax.xsd_regexp =
            regex_ptrs.descendant_schema_nodeid.as_ptr() as *mut _;
        types[IDX_DESCENDANT_SCHEMA_NODEID].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_ABSOLUTE_SCHEMA_NODEID < ntypes {
        types[IDX_ABSOLUTE_SCHEMA_NODEID].name = yang_make_atom(c"absolute-schema-nodeid".as_ptr());
        types[IDX_ABSOLUTE_SCHEMA_NODEID].syntax.xsd_regexp =
            regex_ptrs.absolute_schema_nodeid.as_ptr() as *mut _;
        types[IDX_ABSOLUTE_SCHEMA_NODEID].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_ENUM_ARG < ntypes {
        types[IDX_ENUM_ARG].name = yang_make_atom(c"enum-arg".as_ptr());
        types[IDX_ENUM_ARG].syntax.cb.validate = Some(chk_enum_arg_rs);
        types[IDX_ENUM_ARG].flags = F_ARG_TYPE_SYNTAX_CB;
    }
    if IDX_RANGE_ARG < ntypes {
        types[IDX_RANGE_ARG].name = yang_make_atom(c"range-arg".as_ptr());
        types[IDX_RANGE_ARG].syntax.xsd_regexp = RANGE_ARG_RE.as_ptr() as *mut _;
        types[IDX_RANGE_ARG].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_IDENTIFIER_REF < ntypes {
        types[IDX_IDENTIFIER_REF].name = yang_make_atom(c"identifier-ref".as_ptr());
        types[IDX_IDENTIFIER_REF].syntax.xsd_regexp = IDENTIFIER_REF_RE.as_ptr() as *mut _;
        types[IDX_IDENTIFIER_REF].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_FRACTION_DIGITS_ARG < ntypes {
        types[IDX_FRACTION_DIGITS_ARG].name = yang_make_atom(c"fraction-digits-arg".as_ptr());
        types[IDX_FRACTION_DIGITS_ARG].syntax.cb.validate = Some(chk_fraction_digits_arg_rs);
        types[IDX_FRACTION_DIGITS_ARG].flags = F_ARG_TYPE_SYNTAX_CB;
    }
    if IDX_UNIQUE_ARG < ntypes {
        types[IDX_UNIQUE_ARG].name = yang_make_atom(c"unique-arg".as_ptr());
        types[IDX_UNIQUE_ARG].syntax.xsd_regexp = regex_ptrs.unique_arg.as_ptr() as *mut _;
        types[IDX_UNIQUE_ARG].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_PATH_ARG < ntypes {
        types[IDX_PATH_ARG].name = yang_make_atom(c"path-arg".as_ptr());
        types[IDX_PATH_ARG].syntax.xsd_regexp = regex_ptrs.path_arg.as_ptr() as *mut _;
        types[IDX_PATH_ARG].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_STRICT_PATH_ARG < ntypes {
        types[IDX_STRICT_PATH_ARG].name = yang_make_atom(c"strict-path-arg".as_ptr());
        types[IDX_STRICT_PATH_ARG].syntax.xsd_regexp =
            regex_ptrs.strict_path_arg.as_ptr() as *mut _;
        types[IDX_STRICT_PATH_ARG].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_SCHEMA_NODEID < ntypes {
        types[IDX_SCHEMA_NODEID].name = yang_make_atom(c"schema-nodeid".as_ptr());
        types[IDX_SCHEMA_NODEID].syntax.xsd_regexp = regex_ptrs.schema_nodeid.as_ptr() as *mut _;
        types[IDX_SCHEMA_NODEID].flags = F_ARG_TYPE_SYNTAX_REGEXP;
    }
    if IDX_IF_FEATURE_EXPR < ntypes {
        types[IDX_IF_FEATURE_EXPR].name = yang_make_atom(c"if-feature-expr".as_ptr());
        types[IDX_IF_FEATURE_EXPR].syntax.cb.validate = Some(chk_if_feature_expr_rs);
        types[IDX_IF_FEATURE_EXPR].syntax.cb.opaque =
            xmlRegexpCompile(IDENTIFIER_REF_RE.as_ptr()) as *mut c_void;
        types[IDX_IF_FEATURE_EXPR].flags = F_ARG_TYPE_SYNTAX_CB;
    }
}
