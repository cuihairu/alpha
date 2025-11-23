import React from 'react';
import ComponentCreator from '@docusaurus/ComponentCreator';

export default [
  {
    path: '/alpha/blog',
    component: ComponentCreator('/alpha/blog', 'dd7'),
    exact: true
  },
  {
    path: '/alpha/blog/alpha-finance-launch',
    component: ComponentCreator('/alpha/blog/alpha-finance-launch', '594'),
    exact: true
  },
  {
    path: '/alpha/blog/archive',
    component: ComponentCreator('/alpha/blog/archive', '94a'),
    exact: true
  },
  {
    path: '/alpha/blog/tags',
    component: ComponentCreator('/alpha/blog/tags', 'ae8'),
    exact: true
  },
  {
    path: '/alpha/blog/tags/announcement',
    component: ComponentCreator('/alpha/blog/tags/announcement', '598'),
    exact: true
  },
  {
    path: '/alpha/blog/tags/release',
    component: ComponentCreator('/alpha/blog/tags/release', '67f'),
    exact: true
  },
  {
    path: '/alpha/blog/tags/rust',
    component: ComponentCreator('/alpha/blog/tags/rust', 'ab5'),
    exact: true
  },
  {
    path: '/alpha/blog/tags/webassembly',
    component: ComponentCreator('/alpha/blog/tags/webassembly', 'dd7'),
    exact: true
  },
  {
    path: '/alpha/docs',
    component: ComponentCreator('/alpha/docs', '635'),
    routes: [
      {
        path: '/alpha/docs',
        component: ComponentCreator('/alpha/docs', '7a8'),
        routes: [
          {
            path: '/alpha/docs',
            component: ComponentCreator('/alpha/docs', '5a6'),
            routes: [
              {
                path: '/alpha/docs/',
                component: ComponentCreator('/alpha/docs/', '3fa'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/deployment',
                component: ComponentCreator('/alpha/docs/deployment', '82b'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/getting-started',
                component: ComponentCreator('/alpha/docs/getting-started', '53e'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/installation',
                component: ComponentCreator('/alpha/docs/installation', '78c'),
                exact: true,
                sidebar: "tutorialSidebar"
              }
            ]
          }
        ]
      }
    ]
  },
  {
    path: '*',
    component: ComponentCreator('*'),
  },
];
