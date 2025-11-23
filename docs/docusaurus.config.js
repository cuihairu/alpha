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
        docs: {
          sidebarPath: require.resolve('./sidebars.js'),
          editUrl: 'https://github.com/cuihairu/alpha/tree/main/docs/',
        },
        blog: {
          showReadingTime: true,
          editUrl: 'https://github.com/cuihairu/alpha/tree/main/docs/blog/',
        },
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
          to: '/blog',
          label: '博客',
          position: 'left'
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
              label: '快速开始',
              to: '/docs/intro',
            },
            {
              label: '部署指南',
              to: '/docs/deployment',
            },
            {
              label: 'API 文档',
              to: '/docs/api/overview',
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
              label: '博客',
              to: '/blog',
            },
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