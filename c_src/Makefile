REGEX_NIF = ../priv/w3cregex_nif.so
YANG_PARSER_RUSTLER_NIF = ../priv/yang_parser_rustler_nif.so
RUST_ATOM_LIB = rust_atom/target/release/librust_atom.a
RUSTLER_REGEX_LIB = rustler_w3cregex/target/release/libw3cregex_nif.so
RUSTLER_YANG_PARSER_LIB = rustler_yang_parser/target/release/libyang_parser_rustler_nif.so

PRIV_FILES = $(REGEX_NIF) $(YANG_PARSER_RUSTLER_NIF)

all: $(PRIV_FILES)

$(RUST_ATOM_LIB):
	cargo build --manifest-path rust_atom/Cargo.toml --release

$(RUSTLER_REGEX_LIB):
	cargo build --manifest-path rustler_w3cregex/Cargo.toml --release

$(RUSTLER_YANG_PARSER_LIB): $(RUST_ATOM_LIB)
	cargo build --manifest-path rustler_yang_parser/Cargo.toml --release

$(REGEX_NIF): $(RUSTLER_REGEX_LIB)
	cp $(RUSTLER_REGEX_LIB) $(REGEX_NIF)

$(YANG_PARSER_RUSTLER_NIF): $(RUSTLER_YANG_PARSER_LIB)
	cp $(RUSTLER_YANG_PARSER_LIB) $(YANG_PARSER_RUSTLER_NIF)

debug:
	$(MAKE) DEBUG=true all

clean:
	rm -f ../priv/*.so
	rm -rf rust_atom/target
	rm -rf rustler_w3cregex/target
	rm -rf rustler_yang_parser/target
