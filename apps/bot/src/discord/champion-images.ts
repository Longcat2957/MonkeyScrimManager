import type { Resvg } from '@resvg/resvg-js';

const DATA_DRAGON_ORIGIN = 'https://ddragon.leagueoflegends.com';
const CACHE_LIMIT = 384;
const SUCCESS_TTL_MS = 24 * 60 * 60 * 1_000;
const FAILURE_TTL_MS = 5 * 60 * 1_000;
const REQUEST_TIMEOUT_MS = 2_500;
const MAX_IMAGE_BYTES = 512 * 1024;
const MAX_CONCURRENT_REQUESTS = 12;

const championIdPattern = /^[A-Za-z0-9]+$/;
const patchVersionPattern = /^\d+\.\d+(?:\.\d+)?$/;

interface CachedImage {
	expiresAt: number;
	value: Buffer | null;
}

interface ChampionImageResolverOptions {
	fetcher?: typeof fetch;
	now?: () => number;
}

type ImageResolver = Pick<Resvg, 'imagesToResolve' | 'resolveImage'>;

const imageCache = new Map<string, CachedImage>();
const inFlight = new Map<string, Promise<Buffer | null>>();

/**
 * Data Dragon has patch releases such as 15.14.1 while game data can carry
 * the shorter 15.14 form. Invalid values deliberately render as an empty slot.
 */
export function championImageHref(
	championId: string | null,
	patchVersion: string | null
): string | null {
	if (!championId || !patchVersion || !championIdPattern.test(championId)) return null;

	const version = normalizePatchVersion(patchVersion);
	if (!version) return null;

	return `${DATA_DRAGON_ORIGIN}/cdn/${version}/img/champion/${championId}.png`;
}

export function normalizePatchVersion(value: string): string | null {
	if (!patchVersionPattern.test(value)) return null;
	return value.split('.').length === 2 ? `${value}.1` : value;
}

/**
 * resvg intentionally does not fetch remote image links. Resolve only our
 * fixed Data Dragon URLs, and degrade to the SVG's neutral slot on any error.
 */
export async function resolveChampionImages(
	resvg: ImageResolver,
	options: ChampionImageResolverOptions = {}
): Promise<void> {
	const hrefs = [...new Set(resvg.imagesToResolve().filter(isChampionImageHref))];
	if (!hrefs.length) return;

	const fetcher = options.fetcher ?? fetch;
	const now = options.now ?? Date.now;
	let next = 0;
	const workerCount = Math.min(MAX_CONCURRENT_REQUESTS, hrefs.length);

	await Promise.all(
		Array.from({ length: workerCount }, async () => {
			while (next < hrefs.length) {
				const href = hrefs[next++];
				if (!href) continue;
				const image = await championImage(href, fetcher, now);
				if (!image) continue;
				try {
					resvg.resolveImage(href, image);
				} catch {
					// A failed artwork resolve must never block the result notification.
				}
			}
		})
	);
}

function isChampionImageHref(href: string): boolean {
	try {
		const url = new URL(href);
		return (
			url.origin === DATA_DRAGON_ORIGIN &&
			/^\/cdn\/\d+\.\d+\.\d+\/img\/champion\/[A-Za-z0-9]+\.png$/.test(url.pathname)
		);
	} catch {
		return false;
	}
}

async function championImage(
	href: string,
	fetcher: typeof fetch,
	now: () => number
): Promise<Buffer | null> {
	const cached = imageCache.get(href);
	const currentTime = now();
	if (cached && cached.expiresAt > currentTime) return cached.value;
	if (cached) imageCache.delete(href);

	const pending = inFlight.get(href);
	if (pending) return pending;

	const request = fetchChampionImage(href, fetcher)
		.then((value) => {
			cacheImage(href, value, currentTime + (value ? SUCCESS_TTL_MS : FAILURE_TTL_MS));
			return value;
		})
		.catch(() => {
			cacheImage(href, null, currentTime + FAILURE_TTL_MS);
			return null;
		})
		.finally(() => inFlight.delete(href));

	inFlight.set(href, request);
	return request;
}

async function fetchChampionImage(href: string, fetcher: typeof fetch): Promise<Buffer | null> {
	const controller = new AbortController();
	const timeout = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
	timeout.unref?.();

	try {
		const response = await fetcher(href, {
			headers: { accept: 'image/png' },
			redirect: 'error',
			signal: controller.signal
		});
		if (
			!response.ok ||
			!response.headers.get('content-type')?.toLowerCase().startsWith('image/png')
		) {
			return null;
		}

		const contentLength = Number(response.headers.get('content-length'));
		if (Number.isFinite(contentLength) && contentLength > MAX_IMAGE_BYTES) return null;

		const buffer = Buffer.from(await response.arrayBuffer());
		return buffer.length > 0 && buffer.length <= MAX_IMAGE_BYTES ? buffer : null;
	} catch {
		return null;
	} finally {
		clearTimeout(timeout);
	}
}

function cacheImage(href: string, value: Buffer | null, expiresAt: number): void {
	imageCache.delete(href);
	imageCache.set(href, { value, expiresAt });
	while (imageCache.size > CACHE_LIMIT) {
		const oldest = imageCache.keys().next().value;
		if (!oldest) return;
		imageCache.delete(oldest);
	}
}
