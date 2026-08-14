import { useEffect, useState } from "react";

import { type BuildInfo, loadBuildInfo } from "./build-info";

export function App() {
  const [buildInfo, setBuildInfo] = useState<BuildInfo | undefined>();

  useEffect(() => {
    let active = true;
    void loadBuildInfo().then((value) => {
      if (active) {
        setBuildInfo(value);
      }
    });
    return () => {
      active = false;
    };
  }, []);

  return (
    <main className="app-shell">
      <header className="hero">
        <p className="eyebrow">Material Eagle · Phase 1</p>
        <h1>素材仍在原处，视图由你组织。</h1>
        <p className="summary">
          文件系统是唯一真相源。应用只解释素材与相邻
          Sidecar，不重命名、不搬移，也不把目录结构变成数据库。
        </p>
      </header>

      <section className="status-card" aria-labelledby="status-title">
        <div>
          <p className="section-label">当前里程碑</p>
          <h2 id="status-title">P1-01 工程骨架</h2>
        </div>
        <span className="status-badge">READY</span>
        <dl className="build-grid">
          <BuildField label="版本" value={buildInfo?.version} />
          <BuildField label="提交" value={buildInfo?.gitCommit} />
          <BuildField label="目标" value={buildInfo?.buildTarget} />
          <BuildField label="配置" value={buildInfo?.buildProfile} />
        </dl>
      </section>

      <section className="principles" aria-label="产品约束">
        <article>
          <span>01</span>
          <h2>文件系统优先</h2>
          <p>删除派生缓存后，素材与用户元数据仍能完整重建。</p>
        </article>
        <article>
          <span>02</span>
          <h2>扁平组织</h2>
          <p>物理目录保持不变，Tag 和过滤器提供自由的逻辑视图。</p>
        </article>
        <article>
          <span>03</span>
          <h2>稳定引用</h2>
          <p>Vault 内沿用标准引用，Vault 外通过受限稳定 ID 联动。</p>
        </article>
      </section>
    </main>
  );
}

function BuildField({
  label,
  value,
}: {
  label: string;
  value: string | undefined;
}) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{value ?? "读取中…"}</dd>
    </div>
  );
}
