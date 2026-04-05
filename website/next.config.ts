import createMDX from '@next/mdx';
import type { NextConfig } from 'next';

const nextConfig: NextConfig = {
  output: 'export',
  pageExtensions: ['ts', 'tsx', 'md', 'mdx'],
};

const withMDX = createMDX({});
export default withMDX(nextConfig);
