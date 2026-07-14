import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig, loadEnv } from 'vite';

export default defineConfig(({ mode }) => {
	const env = loadEnv(mode, '../..', 'API_');
	const apiOrigin = new URL(env.API_BASE_URL ?? 'http://127.0.0.1:30020/api/').origin;

	return {
		envDir: '../..',
		plugins: [tailwindcss(), sveltekit()],
		server: {
			proxy: {
				'/ws': {
					target: apiOrigin,
					ws: true
				}
			}
		}
	};
});
