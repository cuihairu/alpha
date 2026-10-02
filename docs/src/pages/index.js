import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';

// 站点首页：此前 src/pages/ 不存在，站根（/alpha/）一直是 404，
// 导航栏 Logo 的站根链接在每页都报死链。本页只做入口导流，不承载内容。

export default function Home() {
  return (
    <Layout title="Alpha 文档站" description="A 股低延迟行情数据与分析平台——工程文档">
      <main
        style={{
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          minHeight: '60vh',
          gap: '1.25rem',
          padding: '2rem',
          textAlign: 'center',
        }}
      >
        <h1>Alpha 文档站</h1>
        <p style={{ maxWidth: '42rem' }}>
          A 股低延迟行情数据与分析平台的工程文档：Rust 核心、Web / 桌面 / Android
          多端、服务端部署与发布流水线。
        </p>
        <div style={{ display: 'flex', gap: '1rem' }}>
          <Link className="button button--primary button--lg" to="/docs/intro">
            从简介开始
          </Link>
          <Link className="button button--secondary button--lg" to="/docs/user-guide">
            用户指南
          </Link>
          <a className="button button--secondary button--lg" href="https://github.com/cuihairu/alpha">
            GitHub 仓库
          </a>
        </div>
      </main>
    </Layout>
  );
}
