import React from 'react';
import ComponentCreator from '@docusaurus/ComponentCreator';

export default [
  {
    path: '/alpha/docs',
    component: ComponentCreator('/alpha/docs', 'a40'),
    routes: [
      {
        path: '/alpha/docs',
        component: ComponentCreator('/alpha/docs', '9a4'),
        routes: [
          {
            path: '/alpha/docs',
            component: ComponentCreator('/alpha/docs', '240'),
            routes: [
              {
                path: '/alpha/docs/',
                component: ComponentCreator('/alpha/docs/', '47f'),
                exact: true
              },
              {
                path: '/alpha/docs/account-sync',
                component: ComponentCreator('/alpha/docs/account-sync', '782'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/alerting-and-diagnosis',
                component: ComponentCreator('/alpha/docs/alerting-and-diagnosis', 'c0e'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/analysis',
                component: ComponentCreator('/alpha/docs/analysis', 'e10'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/android-release',
                component: ComponentCreator('/alpha/docs/android-release', 'e1a'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/android-widget',
                component: ComponentCreator('/alpha/docs/android-widget', 'dcc'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/architecture',
                component: ComponentCreator('/alpha/docs/architecture', '886'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/auth',
                component: ComponentCreator('/alpha/docs/auth', '421'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/auto-update',
                component: ComponentCreator('/alpha/docs/auto-update', '371'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/cross-platform-architecture',
                component: ComponentCreator('/alpha/docs/cross-platform-architecture', '0e4'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/data-lake-parquet',
                component: ComponentCreator('/alpha/docs/data-lake-parquet', '99c'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/data-privacy',
                component: ComponentCreator('/alpha/docs/data-privacy', 'bb6'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/deployment',
                component: ComponentCreator('/alpha/docs/deployment', '8f7'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/deployment-runbook',
                component: ComponentCreator('/alpha/docs/deployment-runbook', 'cad'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/desktop-framework',
                component: ComponentCreator('/alpha/docs/desktop-framework', 'e99'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/desktop-release',
                component: ComponentCreator('/alpha/docs/desktop-release', '3ca'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/distributed-tracing',
                component: ComponentCreator('/alpha/docs/distributed-tracing', '924'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/docker-deployment',
                component: ComponentCreator('/alpha/docs/docker-deployment', 'c0a'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/getting-started',
                component: ComponentCreator('/alpha/docs/getting-started', '168'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/installation',
                component: ComponentCreator('/alpha/docs/installation', '264'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/intro',
                component: ComponentCreator('/alpha/docs/intro', '29e'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/ios-live-activities',
                component: ComponentCreator('/alpha/docs/ios-live-activities', '0c6'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/ios-release',
                component: ComponentCreator('/alpha/docs/ios-release', '944'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/market-data-api',
                component: ComponentCreator('/alpha/docs/market-data-api', 'd3b'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/memory-profiling',
                component: ComponentCreator('/alpha/docs/memory-profiling', '9ad'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/mobile-core-architecture',
                component: ComponentCreator('/alpha/docs/mobile-core-architecture', '4d8'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/mobile-gestures',
                component: ComponentCreator('/alpha/docs/mobile-gestures', '0e3'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/mobile-offline',
                component: ComponentCreator('/alpha/docs/mobile-offline', '1a6'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/mobile-privacy',
                component: ComponentCreator('/alpha/docs/mobile-privacy', 'f46'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/mobile-push-sync',
                component: ComponentCreator('/alpha/docs/mobile-push-sync', 'df5'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/pgo-build-optimization',
                component: ComponentCreator('/alpha/docs/pgo-build-optimization', 'ec9'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/platform-compliance',
                component: ComponentCreator('/alpha/docs/platform-compliance', 'e05'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/realtime-sync-protocol',
                component: ComponentCreator('/alpha/docs/realtime-sync-protocol', '2bd'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/rust-code-standards',
                component: ComponentCreator('/alpha/docs/rust-code-standards', '6a5'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/store-publishing',
                component: ComponentCreator('/alpha/docs/store-publishing', '536'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/testing',
                component: ComponentCreator('/alpha/docs/testing', '8f7'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/theme-adaptation',
                component: ComponentCreator('/alpha/docs/theme-adaptation', '88e'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/user-guide',
                component: ComponentCreator('/alpha/docs/user-guide', 'b28'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/ux-consistency',
                component: ComponentCreator('/alpha/docs/ux-consistency', 'f98'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/version-management',
                component: ComponentCreator('/alpha/docs/version-management', '6ab'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/web-cdn',
                component: ComponentCreator('/alpha/docs/web-cdn', '164'),
                exact: true,
                sidebar: "tutorialSidebar"
              },
              {
                path: '/alpha/docs/web-framework-selection',
                component: ComponentCreator('/alpha/docs/web-framework-selection', 'fd7'),
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
    path: '/alpha/',
    component: ComponentCreator('/alpha/', '64d'),
    exact: true
  },
  {
    path: '*',
    component: ComponentCreator('*'),
  },
];
