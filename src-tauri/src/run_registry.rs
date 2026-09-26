//! `(task_id, generation)` 命名空间已随 P3-01 切片三迁入 core
//! (`aeroric_core::runtime::registry`):它就是 TaskRegistry 的 `runs` 容器,与
//! 其余 13 张表同宅。这里只 re-export——宿主全部 `run_registry::` 路径不变,
//! 锁序表与 ADR-020/025 的规则文档随本体搬进了 core。

#[allow(unused_imports)]
// RunRegistry 只剩 lan_roundtrip_tests 的构造点在用,非测试构建里是重导出空档
pub use aeroric_core::runtime::registry::{RunKey, RunRegistry};
