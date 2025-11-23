/**
 * Creating a sidebar enables you to:
 - create an ordered group of docs
 - render a sidebar for each doc of that group
 - provide next/previous navigation

 The sidebars can be generated from the filesystem, or explicitly defined here.

 Define as many sidebars as you want.
 */

module.exports = {
  tutorialSidebar: [
    {
      type: 'doc',
      id: 'intro',
      label: '简介',
    },
    {
      type: 'category',
      label: '快速开始',
      items: [
        {
          type: 'doc',
          id: 'getting-started',
          label: '环境要求',
        },
        {
          type: 'doc',
          id: 'installation',
          label: '安装指南',
        },
      ],
    },
    {
      type: 'category',
      label: '部署指南',
      items: [
        {
          type: 'doc',
          id: 'deployment',
          label: 'Ubuntu 24.04 部署',
        },
      ],
    },
  ],
};