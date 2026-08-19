# 阶段 0 性能报告

> 日期：2026-08-14
>
> 构建模式：Rust `--release`
>
> 原型版本：提交前工作区

## 测试环境

| 项目 | 值 |
|---|---|
| CPU | Apple M4 |
| 内存 | 16 GiB |
| 操作系统 | macOS 26.5.2（Build 25F84） |
| 文件系统 | APFS |
| 存储 | SSD |
| Rust | 1.97.1 |

## 数据集

测试数据由 `fixture-generator` 以固定规则生成。Sidecar 比例为 20%，包含中文、空格、Emoji、深层路径、零字节素材和损坏 sidecar。

| 数据集 | 素材 | Sidecar |
|---|---:|---:|
| M | 10,000 | 2,000 |
| L | 100,000 | 20,000 |

## 结果

| 指标 | M | L | 阶段基线 | 结论 |
|---|---:|---:|---:|---|
| 夹具生成 | 0.91 s | 5.57 s | 不适用 | 通过 |
| 完整扫描 | 107 ms | 1,454 ms | 60 s | 通过 |
| Tag 查询 p50 | 0.682 ms | 17.151 ms | 不适用 | 通过 |
| Tag 查询 p95 | 0.721 ms | 19.082 ms | 100 ms | 通过 |
| 峰值常驻内存 | 未记录 | 202,473,472 B（约 193 MiB） | 1 GiB | 通过 |

L 数据集运行 1,000 次混合查询，总基准进程耗时 19.02 秒。完整扫描结果包含 100,000 个素材；损坏 sidecar 被隔离到对应素材问题字段，没有中断扫描。

### 2026-08-19 批量事务补测

P2-03 纯文件事务完成后，使用 Release `transaction-fault` 在隔离 APFS 目录创建 100 个素材、生成完整计划日志并原子写入 100 个 Sidecar，总墙钟时间 1.39 秒（user 0.04 秒、sys 0.12 秒）。该保守结果包含测试素材创建，满足“100 个素材批量 Tag 写入不超过 3 秒”的阶段基线。

## 使用命令

```bash
cargo build --release -p eagle-p0 -p fixture-generator

./target/release/fixture-generator generate /tmp/eagle-perf-l --scale large
/usr/bin/time -l ./target/release/eagle-p0 benchmark /tmp/eagle-perf-l --iterations 1000
./target/release/fixture-generator clean /tmp/eagle-perf-l
```

## 尚未完成的性能验收

- UI 尚未实现，因此“首屏可交互”目前只能以完整扫描 1.454 秒作为阶段 0 的保守替代证据；
- 文件事件 p95 只完成单文件烟测，10,000 文件事件风暴待执行；
- 空闲 CPU 属于阶段 1 桌面应用指标，CLI 原型不适用。
