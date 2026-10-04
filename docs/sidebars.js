// 侧边栏 = docs/ 根下全部 42 篇文档的分类树。
// 约定：内容与文件都在 docs/ 根（docusaurus.config.js 的 docs.path = '.'），
// 新增文档后在此登记到对应分类；未登记的文档会成为 orphan（构建仅警告）。

module.exports = {
  tutorialSidebar: [
    {
      type: 'doc',
      id: 'intro',
      label: '简介',
    },
    {
      type: 'category',
      label: '入门',
      items: [
        { type: 'doc', id: 'getting-started', label: '环境要求' },
        { type: 'doc', id: 'installation', label: '安装指南' },
      ],
    },
    {
      type: 'doc',
      id: 'user-guide',
      label: '用户指南',
    },
    {
      type: 'category',
      label: '架构与设计',
      items: [
        { type: 'doc', id: 'architecture', label: '平台总体方案' },
        { type: 'doc', id: 'architecture-review', label: '架构收敛审查' },
        { type: 'doc', id: 'cross-platform-architecture', label: '跨平台 Rust 架构' },
        { type: 'doc', id: 'web-framework-selection', label: 'UI 框架选型决策' },
        { type: 'doc', id: 'desktop-framework', label: '桌面端框架（Tauri）' },
        { type: 'doc', id: 'mobile-core-architecture', label: '移动端核心库（JNI + UniFFI）' },
        { type: 'doc', id: 'analysis', label: '现状分析（早期规划）' },
      ],
    },
    {
      type: 'category',
      label: '服务端',
      items: [
        { type: 'doc', id: 'market-data-api', label: '市场数据 API' },
        { type: 'doc', id: 'auth', label: '身份认证' },
        { type: 'doc', id: 'account-sync', label: '统一账户与跨端同步' },
        { type: 'doc', id: 'realtime-sync-protocol', label: '实时同步协议' },
        { type: 'doc', id: 'data-lake-parquet', label: 'Parquet 数据湖' },
        { type: 'doc', id: 'distributed-tracing', label: '分布式追踪' },
        { type: 'doc', id: 'alerting-and-diagnosis', label: '告警与故障诊断' },
        { type: 'doc', id: 'memory-profiling', label: '内存与性能分析' },
        { type: 'doc', id: 'pgo-build-optimization', label: 'PGO 编译优化' },
      ],
    },
    {
      type: 'category',
      label: '部署与发布',
      items: [
        { type: 'doc', id: 'deployment', label: '部署指南（总览）' },
        { type: 'doc', id: 'deployment-runbook', label: 'Ubuntu 部署 Runbook' },
        { type: 'doc', id: 'docker-deployment', label: 'Docker 容器化' },
        { type: 'doc', id: 'web-cdn', label: 'Web CDN 与静态资源' },
        { type: 'doc', id: 'desktop-release', label: '桌面多平台安装包' },
        { type: 'doc', id: 'auto-update', label: '自动更新' },
        { type: 'doc', id: 'android-release', label: 'Android 发布' },
        { type: 'doc', id: 'ios-release', label: 'iOS 签名与发布' },
        { type: 'doc', id: 'store-publishing', label: '应用商店发布' },
        { type: 'doc', id: 'version-management', label: '版本管理' },
      ],
    },
    {
      type: 'category',
      label: '移动端体验',
      items: [
        { type: 'doc', id: 'mobile-offline', label: '离线存储与同步' },
        { type: 'doc', id: 'mobile-push-sync', label: '推送通知与后台同步' },
        { type: 'doc', id: 'mobile-gestures', label: '触屏手势与交互' },
        { type: 'doc', id: 'mobile-privacy', label: '生物识别门与隐私' },
        { type: 'doc', id: 'android-widget', label: 'Android 小组件' },
        { type: 'doc', id: 'ios-live-activities', label: 'iOS 灵动岛' },
      ],
    },
    {
      type: 'category',
      label: '跨端体验一致性',
      items: [
        { type: 'doc', id: 'ux-consistency', label: 'UX 一致性' },
        { type: 'doc', id: 'theme-adaptation', label: '深色模式与主题' },
      ],
    },
    {
      type: 'category',
      label: '安全与合规',
      items: [
        { type: 'doc', id: 'platform-compliance', label: '平台合规与权限' },
        { type: 'doc', id: 'data-privacy', label: '数据合规（GDPR/CCPA）' },
      ],
    },
    {
      type: 'category',
      label: '测试与质量',
      items: [
        { type: 'doc', id: 'testing', label: '测试体系' },
        { type: 'doc', id: 'rust-code-standards', label: 'Rust 代码规范' },
      ],
    },
  ],
};
