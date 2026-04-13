use std::fs;
use std::path::PathBuf;

fn escape_rust(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let c_file = manifest_dir.join("core_stmts_data.txt");
    println!("cargo:rerun-if-changed={}", c_file.display());

    let src = fs::read_to_string(&c_file).expect("read core_stmts_data.txt");
    let start = src
        .find("static const char *stmts[] = {")
        .expect("find stmts start");
    let rest = &src[start..];
    let end_rel = rest.find("\n};").expect("find stmts end");
    let block = &rest[..end_rel];

    let mut entries: Vec<Option<String>> = Vec::new();
    for line in block.lines().skip(1) {
        let mut in_str = false;
        let mut cur = String::new();
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let ch = bytes[i] as char;
            if in_str {
                if ch == '"' {
                    entries.push(Some(cur.clone()));
                    cur.clear();
                    in_str = false;
                } else {
                    cur.push(ch);
                }
            } else if ch == '"' {
                in_str = true;
            }
            i += 1;
        }
        let mut scan = line;
        while let Some(pos) = scan.find("NULL") {
            entries.push(None);
            scan = &scan[pos + 4..];
        }
    }

    let mut out = String::from("pub static CORE_STMTS: &[Option<&str>] = &[\n");
    for e in entries {
        match e {
            Some(s) => {
                out.push_str("    Some(\"");
                out.push_str(&escape_rust(&s));
                out.push_str("\"),\n");
            }
            None => out.push_str("    None,\n"),
        }
    }
    out.push_str("];\n");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out_dir.join("core_stmts_generated.rs"), out).expect("write generated rs");
}
