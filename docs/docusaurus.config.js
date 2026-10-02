// @ts-check
/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'Alpha Finance',
  tagline: '高性能 Rust + WebAssembly 金融数据分析平台',
  url: 'https://cuihairu.github.io',
  baseUrl: '/alpha/',
  organizationName: 'cuihairu',
  projectName: 'alpha',

  onBrokenLinks: 'warn',
  onBrokenMarkdownLinks: 'warn',

  i18n: {
    defaultLocale: 'zh-CN',
    locales: ['zh-CN'],
  },

  presets: [
    [
      'classic',
      {
        // 文档内容就放在 docs/ 根（42 篇工程文档），不再维护 docs/docs/ 子树。
        // path 相对 siteDir（docs/）；构建产物/依赖/生成目录必须显式排除——
        // path='.' 时 contentDir=siteDir，否则 node_modules 里的 README 会被当成文档。
        docs: {
          path: '.',
          exclude: ['node_modules/**', 'build/**', '.docusaurus/**', 'src/**', 'static/**'],
          sidebarPath: require.resolve('./sidebars.js'),
          editUrl: 'https://github.com/cuihairu/alpha/tree/main/docs/',
        },
        // 站内无真实博客内容，停用（删除原占位文章后不再启用）。
        blog: false,
        theme: {
          customCss: require.resolve('./src/css/custom.css'),
        },
      },
    ],
  ],

  themeConfig: {
    navbar: {
      title: 'Alpha Finance',
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'tutorialSidebar',
          position: 'left',
          label: '文档',
        },
        {
          href: 'https://github.com/cuihairu/alpha',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: '文档',
          items: [
            {
              label: '简介',
              to: '/docs/intro',
            },
            {
              label: '快速开始',
              to: '/docs/getting-started',
            },
            {
              label: '部署指南',
              to: '/docs/deployment',
            },
            {
              label: '市场数据 API',
              to: '/docs/market-data-api',
            },
          ],
        },
        {
          title: '社区',
          items: [
            {
              label: 'GitHub',
              href: 'https://github.com/cuihairu/alpha',
            },
            {
              label: 'Issues',
              href: 'https://github.com/cuihairu/alpha/issues',
            },
            {
              label: 'Discussions',
              href: 'https://github.com/cuihairu/alpha/discussions',
            },
          ],
        },
        {
          title: '更多',
          items: [
            {
              label: '更新日志',
              href: 'https://github.com/cuihairu/alpha/releases',
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Alpha Finance Team. Built with Rust ❤️.`,
    },
  },
};

module.exports = config;
