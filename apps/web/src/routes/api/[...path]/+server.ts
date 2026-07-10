import { env } from '$env/dynamic/private';
import type { RequestHandler } from './$types';

type ProxyRequestInit = RequestInit & { duplex?: 'half' };

const hopByHopHeaders = new Set([
	'connection',
	'keep-alive',
	'proxy-authenticate',
	'proxy-authorization',
	'te',
	'trailer',
	'transfer-encoding',
	'upgrade'
]);

function forwardHeaders(headers: Headers): Headers {
	const forwarded = new Headers(headers);
	for (const header of hopByHopHeaders) {
		forwarded.delete(header);
	}
	return forwarded;
}

export const fallback: RequestHandler = async ({ request, params, url, fetch }) => {
	const baseUrl = env.API_BASE_URL;

	if (!baseUrl) {
		return new Response('API_BASE_URL is not configured.', { status: 503 });
	}

	const upstream = new URL(params.path ?? '', baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`);
	upstream.search = url.search;

	const init: ProxyRequestInit = {
		method: request.method,
		headers: forwardHeaders(request.headers),
		body: request.body,
		duplex: 'half'
	};

	return fetch(upstream, init);
};
