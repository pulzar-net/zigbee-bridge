// The zigbee-bridge docs: Starlight with the shared Pulzar theme
// (github.com/pulzar-net/docs-theme). Pages are in src/content/docs; the
// configuration reference there is generated from the code
// (crates/zigbee-bridge/tests/config_reference.rs). Built and published by
// .github/workflows/docs.yml, which adds the API docs under /api.
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { pulzarPlugins } from '@pulzar-net/docs-theme';
import sidebar from './sidebar.mjs';

export default defineConfig({
	site: 'https://pulzar-net.github.io',
	base: '/zigbee-bridge',
	// Pages are page.html, the URLs the earlier mdBook site had.
	build: { format: 'file' },
	redirects: {
		'/library/index': '/zigbee-bridge/library.html',
		'/library/zigbee-types': '/zigbee-bridge/library/zigbee-primitives.html',
	},
	integrations: [
		starlight({
			title: 'zigbee-bridge',
			description: 'A Zigbee coordinator stack and Zigbee to MQTT bridge in Rust.',
			plugins: pulzarPlugins({
				project: 'zigbee-bridge',
				linkExclude: ['/zigbee-bridge/api/**', '/zigbee-bridge/config.schema.json'],
			}),
			sidebar,
		}),
	],
});
