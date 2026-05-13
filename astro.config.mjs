import { defineConfig } from 'astro/config';
import remarkRustCodeblock from './src/plugins/remark-rust-codeblock.mjs';
import remarkBoldClass from './src/plugins/remark-bold-class.mjs';

export default defineConfig({
  site: 'https://xyfx-fhw.github.io',
  base: '/RustRTOS',
  markdown: {
    remarkPlugins: [remarkRustCodeblock, remarkBoldClass],
  },
});
