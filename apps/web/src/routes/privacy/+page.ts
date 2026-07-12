import { marked } from 'marked';
import privacyMarkdown from '$lib/content/privacy.md?raw';
import type { PageLoad } from './$types';

export const prerender = true;

export const load: PageLoad = () => {
	return {
		content: marked.parse(privacyMarkdown, { async: false })
	};
};
