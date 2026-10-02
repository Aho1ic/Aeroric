//! P7-01：连接、基础设施、数据库与笔记模块（core 独占；SSH/SFTP 进程由宿主
//! 管理，本模块承载连接配置格式、协议形状、纯校验与队列簿记）。
pub mod sftp;
pub mod ssh;
