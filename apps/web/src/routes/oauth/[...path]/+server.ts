import { env } from '$env/dynamic/private';
import type { RequestHandler } from './$types';

type ProxyRequestInit = RequestInit & { duplex?: 'half' };

export const fallback: RequestHandler = async ({ request, params, url, fetch }) => {
	const baseUrl = env.OAUTH_BASE_URL ?? env.API_BASE_URL;

	if (!baseUrl) {
		return new Response('OAUTH_BASE_URL or API_BASE_URL is not configured.', { status: 503 });
	}

	const upstream = new URL(
		`oauth/${params.path ?? ''}`,
		baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`
	);
	upstream.search = url.search;

	const init: ProxyRequestInit = {
		method: request.method,
		headers: request.headers,
		body: request.body,
		duplex: 'half'
	};

	return fetch(upstream, init);
};
