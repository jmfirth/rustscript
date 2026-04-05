import createMDX from '@next/mdx';
import rehypeShiki from '@shikijs/rehype';
import type { NextConfig } from 'next';

const nextConfig: NextConfig = {
  output: 'export',
  pageExtensions: ['ts', 'tsx', 'md', 'mdx'],
  webpack(config) {
    // Enable WASM support for rustscript-web
    config.experiments = {
      ...config.experiments,
      asyncWebAssembly: true,
    };
    return config;
  },
};

const withMDX = createMDX({
  options: {
    rehypePlugins: [
      [rehypeShiki, {
        themes: {
          light: 'github-light',
          dark: 'github-dark',
        },
        defaultColor: false,
      }],
    ],
  },
});

export default withMDX(nextConfig);
