import { resolve } from '$app/paths';

type QueryValue = string | number;

export function apiRoute(
	segments: readonly string[],
	query: Readonly<Record<string, QueryValue>> = {}
): string {
	const path = segments.map((segment) => encodeURIComponent(segment)).join('/');
	const route = resolve('/api/[...path]', { path });
	const search = new URLSearchParams(
		Object.entries(query).map(([key, value]) => [key, String(value)])
	);
	return search.size > 0 ? `${route}?${search}` : route;
}
