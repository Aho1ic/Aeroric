# Codex app-server protocol snapshot

Generated with `codex app-server generate-json-schema --out <dir>` from the version in `VERSION`
(ADR-043, task N2). Kept: the v2 aggregate schema and the three method lists the native driver
relies on. Regenerate on a Codex upgrade and rerun `cargo test -p aeroric-core sessions::live`;
`codex_methods_exist_in_the_snapshot` fails if a method the driver calls disappeared.
