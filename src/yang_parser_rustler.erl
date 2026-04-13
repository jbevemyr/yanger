-module(yang_parser_rustler).

-export([get_grammar_module_names/0, get_statement_spec/1,
         install_arg_types/1, install_grammar/2, parse/2]).
-on_load(init/0).

get_grammar_module_names() ->
    erlang:nif_error(nif_library_not_loaded).

get_statement_spec(_Keyword) ->
    erlang:nif_error(nif_library_not_loaded).

install_arg_types(_Types) ->
    erlang:nif_error(nif_library_not_loaded).

install_grammar(_ModuleName, _Specs) ->
    erlang:nif_error(nif_library_not_loaded).

parse(_FileName, _Canonical) ->
    erlang:nif_error(nif_library_not_loaded).

init() ->
    Nif = filename:join(code:priv_dir(yanger), "yang_parser_rustler_nif"),
    ok = erlang:load_nif(Nif, 0).
