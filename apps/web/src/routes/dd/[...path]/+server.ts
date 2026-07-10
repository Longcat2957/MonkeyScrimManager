import { env } from '$env/dynamic/private';
import type { RequestHandler } from './$types';

function cacheHeader(path: string): string {
	if (/^cdn\/[^/]+\//.test(path) && !path.startsWith('cdn/img/')) {
		return 'public, max-age=31536000, immutable';
	}

	return 'public, max-age=300, stale-while-revalidate=86400';
}

export const GET: RequestHandler = async ({ params, url, fetch }) => {
	const baseUrl = env.DATA_DRAGON_BASE_URL ?? 'https://ddragon.leagueoflegends.com';
	const upstream = new URL(params.path ?? '', baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`);
	upstream.search = url.search;

	const response = await fetch(upstream);
	const headers = new Headers(response.headers);
	headers.set('cache-control', cacheHeader(params.path ?? ''));
	headers.set('vary', 'Accept-Encoding');

	return new Response(response.body, {
		status: response.status,
		statusText: response.statusText,
		headers
	});
};
