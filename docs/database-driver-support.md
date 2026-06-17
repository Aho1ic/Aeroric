# Database Driver Support

| Driver | Phase | Runtime | Enabled by default | Notes |
| --- | --- | --- | --- | --- |
| SQLite | 1 | native/file | yes | Must preserve old Aeroric local and SSH project behavior |
| PostgreSQL | 2 | native | yes | First network database target |
| MySQL | 2 | native | yes | First network database target |
| DuckDB | 3 | native/file | yes | File database, large binary dependency check required |
| Redis | 5 | native | yes | Dedicated key browser, not SQL schema browser |
| MongoDB | 5 | native | yes | Dedicated document browser |
| SQL Server | 7 | native/JDBC fallback | optional | Validate TLS and auth UX |
| Oracle | 7 | JDBC/native fallback | optional | Driver distribution and license review required |
| ClickHouse | 7 | native | optional | Read/write capability differs from OLTP databases |

## License Notes

DBX root `LICENSE`, `package.json`, and `crates/dbx-core/Cargo.toml` currently declare Apache-2.0. Before vendoring DBX code or enabling redistributable database drivers, review transitive dependency licenses and driver distribution terms.
